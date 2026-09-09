mod command_snapshots;
mod kinds;

use serde_json::{Map, Value};

use crate::acp::{
    contract::{ItemLifecycle, RequiredText, SafeText, ToolState},
    intent::{NormalizedPayload, SafeResourceLocation},
    limits::LIMITS,
    runtime::host_browse::browse_tool_summary,
};

use super::{ActiveTurn, OmpTranslator, ResourceMapper, TranslationError};
use crate::acp::bindings::omp::{
    protocol::{AdapterEvent, ItemUpdateMode},
    wire::{normalized_text, opaque_id, sanitized_command_output},
};

use command_snapshots::{CommandPatch, command_update_payload};
pub(crate) use kinds::{category_label, external_id, object, tool_category};
use kinds::{lifecycle, line_count, optional_item_id, parse_tool_state, required_string};

const MAX_EVENTS_PER_FRAME: usize = 16;
const MAX_PLAN_ENTRIES: usize = 128;
const MAX_TRACKED_TOOLS: usize = 512;

impl OmpTranslator {
    pub(super) fn translate_update(
        &mut self,
        frame: &Value,
        resources: &impl ResourceMapper,
    ) -> Result<Vec<AdapterEvent>, TranslationError> {
        let params = object(frame.get("params"))?;
        self.validate_session(params.get("sessionId"))?;
        if self
            .active
            .as_ref()
            .is_some_and(|turn| turn.terminal.is_some())
        {
            return Ok(vec![AdapterEvent::IgnoredLate]);
        }
        let update = object(params.get("update"))?;
        let kind = required_string(update, "sessionUpdate")?;
        let running = self.active.as_ref().ok_or(TranslationError::StaleIntent)?;
        let running = (!running.running_emitted).then(|| AdapterEvent::TurnRunning {
            turn_key: running.turn_key.clone(),
        });
        let mut events = match kind {
            "agent_message_chunk" => vec![text_event(update, false)?],
            "agent_thought_chunk" => vec![text_event(update, true)?],
            "plan" => vec![plan_event(update)?],
            "tool_call" => self.tool_call(update, resources)?,
            "tool_call_update" => self.tool_update(update, resources)?,
            "usage" | "usage_update" => vec![usage_event(update)?],
            "error" => vec![error_event(update)?],
            "browser" | "lsp" | "dap" | "mcp" | "subagent" => vec![unsupported_event(
                kind,
                "OMP capability metadata is display-only and grants no Sage authority",
            )?],
            other => vec![unsupported_event(
                other,
                "Unsupported bounded OMP ACP update",
            )?],
        };
        if let Some(running) = running {
            self.active
                .as_mut()
                .ok_or(TranslationError::StaleIntent)?
                .running_emitted = true;
            events.insert(0, running);
        }
        if events.len() > MAX_EVENTS_PER_FRAME {
            return Err(TranslationError::Limit("normalized events per frame"));
        }
        Ok(events)
    }

    pub(super) fn validate_session(&self, value: Option<&Value>) -> Result<(), TranslationError> {
        let incoming = opaque_id(value)?;
        if self.session_id.as_ref().map(RequiredText::as_str) == Some(incoming.as_str()) {
            Ok(())
        } else {
            Err(TranslationError::Correlation)
        }
    }

    fn tool_call(
        &mut self,
        update: &Map<String, Value>,
        resources: &impl ResourceMapper,
    ) -> Result<Vec<AdapterEvent>, TranslationError> {
        let id = external_id(update.get("toolCallId"))?;
        let state = parse_tool_state(update.get("status"), ToolState::Pending)?;
        let category = tool_category(update);
        {
            let active = self.live_turn()?;
            if let Some(current) = active.tools.get(id.as_str()).copied() {
                return if current == state {
                    Ok(vec![AdapterEvent::IgnoredDuplicate])
                } else {
                    Err(TranslationError::ToolTransition)
                };
            }
            if active.tools.len() >= MAX_TRACKED_TOOLS
                || active.tools.len() as u64 >= LIMITS.wire.items_per_turn
            {
                return Err(TranslationError::Limit("tools per Turn"));
            }
        }
        let command = category == crate::acp::intent::ToolCategory::Command;
        let snapshot = command
            .then(|| command_update_payload(update, resources))
            .transpose()?
            .map(|patch| patch.apply((None, Default::default())));
        if let Some(snapshot) = snapshot.as_ref() {
            self.validate_command_snapshot(id.as_str(), snapshot)?;
        }
        {
            let active = self.live_turn()?;
            active.tools.insert(id.as_str().to_owned(), state);
            active
                .tool_categories
                .insert(id.as_str().to_owned(), category);
        }
        if let Some(snapshot) = snapshot {
            self.store_command_snapshot(id.as_str(), snapshot)?;
        }
        let (output_summary, locations) = self.command_snapshot(id.as_str())?;
        let mut events = vec![AdapterEvent::Item {
            external_key: id.clone(),
            lifecycle: lifecycle(state),
            tool_state: Some(state),
            payload: NormalizedPayload::ToolCall {
                category,
                label: SafeText::new("toolLabel", category_label(category))?,
                input_summary: browse_tool_summary("toolInput", update),
                output_summary: command.then_some(output_summary).flatten(),
                locations: if command {
                    locations
                } else {
                    Default::default()
                },
            },
            update_mode: ItemUpdateMode::Replace,
        }];
        // The H4 first slice grants no OMP tool authority. Record only the
        // bounded presentation and force the containing supervisor to cancel.
        let dispatch_cancel = {
            let active = self.live_turn()?;
            if active.cancel_dispatch_required {
                false
            } else {
                active.cancel_dispatch_required = true;
                true
            }
        };
        events.push(AdapterEvent::ToolAuthorityDenied { tool_call_id: id });
        if dispatch_cancel {
            events.push(AdapterEvent::CancelDispatchRequired);
        }
        Ok(events)
    }

    fn tool_update(
        &mut self,
        update: &Map<String, Value>,
        resources: &impl ResourceMapper,
    ) -> Result<Vec<AdapterEvent>, TranslationError> {
        let id = external_id(update.get("toolCallId"))?;
        let current = self
            .live_turn()?
            .tools
            .get(id.as_str())
            .copied()
            .ok_or(TranslationError::Correlation)?;
        let category = self
            .live_turn()?
            .tool_categories
            .get(id.as_str())
            .copied()
            .ok_or(TranslationError::Correlation)?;
        let next = parse_tool_state(update.get("status"), current)?;
        let patch = (category == crate::acp::intent::ToolCategory::Command)
            .then(|| command_update_payload(update, resources))
            .transpose()?;
        if next == current && patch.as_ref().is_none_or(CommandPatch::is_omitted) {
            return Ok(vec![AdapterEvent::IgnoredDuplicate]);
        }
        if next != current && !current.can_transition_to(next) {
            return Err(TranslationError::ToolTransition);
        }
        let command_snapshot = if let Some(patch) = patch {
            let snapshot = self.command_snapshot(id.as_str())?;
            let merged = patch.apply(snapshot);
            self.validate_command_snapshot(id.as_str(), &merged)?;
            Some(merged)
        } else {
            None
        };
        let (output_summary, locations) = if let Some(snapshot) = command_snapshot.as_ref() {
            snapshot.clone()
        } else {
            (
                browse_tool_summary("toolOutput", update),
                Default::default(),
            )
        };
        let mut events = vec![AdapterEvent::Item {
            external_key: id.clone(),
            lifecycle: lifecycle(next),
            tool_state: Some(next),
            payload: NormalizedPayload::ToolUpdateResult {
                category,
                label: SafeText::new("toolLabel", category_label(category))?,
                output_summary,
                locations,
            },
            update_mode: ItemUpdateMode::Replace,
        }];
        if let Some(content) = update.get("content").and_then(Value::as_array) {
            if content.len() >= MAX_EVENTS_PER_FRAME {
                return Err(TranslationError::Limit("tool content entries"));
            }
            for (index, entry) in content.iter().enumerate() {
                if entry.get("type").and_then(Value::as_str) != Some("diff") {
                    continue;
                }
                let path = entry
                    .get("path")
                    .and_then(Value::as_str)
                    .ok_or(TranslationError::MalformedFrame)?;
                let resource = match resources.map_absolute(path) {
                    Ok(resource) => crate::acp::intent::SafeResourceLocation::Resource { resource },
                    Err(reason) => crate::acp::intent::SafeResourceLocation::unavailable(reason),
                };
                // Missing, null, or non-string sides are malformed wire
                // input, not an empty edit. Over-cap sides admit
                // summary-only metadata without retaining a partial side.
                let old_text = entry
                    .get("oldText")
                    .and_then(Value::as_str)
                    .ok_or(TranslationError::MalformedFrame)?;
                let new_text = entry
                    .get("newText")
                    .and_then(Value::as_str)
                    .ok_or(TranslationError::MalformedFrame)?;
                let external_key =
                    RequiredText::new("externalItemId", format!("{}:diff:{index}", id.as_str()))?;
                let summary = SafeText::new("fileSummary", "File change reported by OMP ACP")?;
                // An unavailable resource has no immutable root/path key,
                // so no pair is retained: typed unavailable, never text
                // invented from disk or Git.
                if !matches!(
                    resource,
                    crate::acp::intent::SafeResourceLocation::Resource { .. }
                ) {
                    events.push(AdapterEvent::Item {
                        external_key,
                        lifecycle: ItemLifecycle::Completed,
                        tool_state: None,
                        payload: NormalizedPayload::file_summary_only(
                            resource,
                            summary,
                            line_count(Some(new_text)),
                            line_count(Some(old_text)),
                            false,
                        ),
                        update_mode: ItemUpdateMode::Replace,
                    });
                    continue;
                }
                match crate::acp::file_diff::admit_pair(old_text, new_text) {
                    crate::acp::file_diff::PairAdmission::Admitted {
                        old_bytes,
                        new_bytes,
                        digest,
                    } => {
                        events.push(AdapterEvent::Item {
                            external_key: external_key.clone(),
                            lifecycle: ItemLifecycle::Completed,
                            tool_state: None,
                            payload: NormalizedPayload::file_admitted(
                                resource,
                                summary,
                                line_count(Some(new_text)),
                                line_count(Some(old_text)),
                                0,
                                (old_bytes as u64, new_bytes as u64),
                                digest,
                            ),
                            update_mode: ItemUpdateMode::Replace,
                        });
                        events.push(AdapterEvent::FilePair {
                            external_key,
                            old_text: old_text.to_owned(),
                            new_text: new_text.to_owned(),
                            update_mode: ItemUpdateMode::Replace,
                        });
                    }
                    crate::acp::file_diff::PairAdmission::SummaryOnly => {
                        events.push(AdapterEvent::Item {
                            external_key,
                            lifecycle: ItemLifecycle::Completed,
                            tool_state: None,
                            payload: NormalizedPayload::file_summary_only(
                                resource,
                                summary,
                                line_count(Some(new_text)),
                                line_count(Some(old_text)),
                                true,
                            ),
                            update_mode: ItemUpdateMode::Replace,
                        });
                    }
                }
            }
        }
        self.live_turn()?.tools.insert(id.as_str().to_owned(), next);
        if let Some(snapshot) = command_snapshot {
            self.store_command_snapshot(id.as_str(), snapshot)?;
        }
        Ok(events)
    }

    fn live_turn(&mut self) -> Result<&mut ActiveTurn, TranslationError> {
        let turn = self.active.as_mut().ok_or(TranslationError::StaleIntent)?;
        if turn.terminal.is_some() {
            Err(TranslationError::StaleIntent)
        } else {
            Ok(turn)
        }
    }
}

fn text_event(
    update: &Map<String, Value>,
    thought: bool,
) -> Result<AdapterEvent, TranslationError> {
    let content = object(update.get("content"))?;
    if required_string(content, "type")? != "text" {
        return unsupported_event("non_text_content", "Non-text OMP content is not admitted");
    }
    let text = normalized_text::<262144>("text", required_string(content, "text")?)?;
    Ok(AdapterEvent::Item {
        external_key: optional_item_id(update, if thought { "thought" } else { "agent-message" })?,
        lifecycle: ItemLifecycle::Streaming,
        tool_state: None,
        payload: if thought {
            NormalizedPayload::ThoughtSummary { summary: text }
        } else {
            NormalizedPayload::Text { text }
        },
        update_mode: ItemUpdateMode::Append,
    })
}

mod progress;
use progress::{plan_event, usage_event};

fn error_event(update: &Map<String, Value>) -> Result<AdapterEvent, TranslationError> {
    let retryable = update
        .get("retryable")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    Ok(AdapterEvent::Item {
        external_key: optional_item_id(update, "error")?,
        lifecycle: ItemLifecycle::Failed,
        tool_state: None,
        payload: NormalizedPayload::Error {
            code: SafeText::new("errorCode", "omp_acp_error")?,
            message: SafeText::new("errorMessage", "OMP reported a bounded ACP error")?,
            retryable,
        },
        update_mode: ItemUpdateMode::Replace,
    })
}

pub(super) fn unsupported_event(
    kind: &str,
    summary: &str,
) -> Result<AdapterEvent, TranslationError> {
    let kind = normalized_text::<128>("externalKind", kind)?;
    Ok(AdapterEvent::Item {
        external_key: RequiredText::new(
            "externalItemId",
            format!("unsupported:{}", kind.as_str()),
        )?,
        lifecycle: ItemLifecycle::Unsupported,
        tool_state: None,
        payload: NormalizedPayload::Unsupported {
            external_kind: kind,
            summary: SafeText::new("unsupportedSummary", summary)?,
        },
        update_mode: ItemUpdateMode::Replace,
    })
}
