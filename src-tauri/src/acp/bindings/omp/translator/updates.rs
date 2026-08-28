mod kinds;

use serde_json::{Map, Value};

use crate::acp::{
    contract::{ItemLifecycle, RequiredText, SafeText, ToolState},
    intent::NormalizedPayload,
    limits::LIMITS,
    runtime::host_browse::browse_tool_summary,
};

use super::{ActiveTurn, OmpTranslator, ResourceMapper, TranslationError};
use crate::acp::bindings::omp::{
    protocol::{AdapterEvent, ItemUpdateMode},
    wire::{normalized_text, opaque_id},
};

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
            "tool_call" => self.tool_call(update)?,
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
    ) -> Result<Vec<AdapterEvent>, TranslationError> {
        let id = external_id(update.get("toolCallId"))?;
        let state = parse_tool_state(update.get("status"), ToolState::Pending)?;
        let category = tool_category(update);
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
        active.tools.insert(id.as_str().to_owned(), state);
        let mut events = vec![AdapterEvent::Item {
            external_key: id.clone(),
            lifecycle: lifecycle(state),
            tool_state: Some(state),
            payload: NormalizedPayload::ToolCall {
                category,
                label: SafeText::new("toolLabel", category_label(category))?,
                input_summary: browse_tool_summary("toolInput", update),
            },
            update_mode: ItemUpdateMode::Replace,
        }];
        // The H4 first slice grants no OMP tool authority. Record only the
        // bounded presentation and force the containing supervisor to cancel.
        events.push(AdapterEvent::ToolAuthorityDenied { tool_call_id: id });
        if !active.cancel_dispatch_required {
            active.cancel_dispatch_required = true;
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
        let category = tool_category(update);
        let current = self
            .live_turn()?
            .tools
            .get(id.as_str())
            .copied()
            .ok_or(TranslationError::Correlation)?;
        let next = parse_tool_state(update.get("status"), current)?;
        if next == current {
            return Ok(vec![AdapterEvent::IgnoredDuplicate]);
        }
        if !current.can_transition_to(next) {
            return Err(TranslationError::ToolTransition);
        }
        let mut events = vec![AdapterEvent::Item {
            external_key: id.clone(),
            lifecycle: lifecycle(next),
            tool_state: Some(next),
            payload: NormalizedPayload::ToolUpdateResult {
                category,
                label: SafeText::new("toolLabel", category_label(category))?,
                output_summary: browse_tool_summary("toolOutput", update),
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
                events.push(AdapterEvent::Item {
                    external_key: RequiredText::new(
                        "externalItemId",
                        format!("{}:diff:{index}", id.as_str()),
                    )?,
                    lifecycle: ItemLifecycle::Completed,
                    tool_state: None,
                    payload: NormalizedPayload::FileChangeDiff {
                        resource,
                        summary: SafeText::new("fileSummary", "File change reported by OMP ACP")?,
                        additions: line_count(entry.get("newText").and_then(Value::as_str)),
                        deletions: line_count(entry.get("oldText").and_then(Value::as_str)),
                    },
                    update_mode: ItemUpdateMode::Replace,
                });
            }
        }
        self.live_turn()?.tools.insert(id.as_str().to_owned(), next);
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

fn plan_event(update: &Map<String, Value>) -> Result<AdapterEvent, TranslationError> {
    let entries = update
        .get("entries")
        .and_then(Value::as_array)
        .ok_or(TranslationError::MalformedFrame)?;
    if entries.len() > MAX_PLAN_ENTRIES {
        return Err(TranslationError::Limit("plan entries"));
    }
    let mut summary = String::new();
    for (index, entry) in entries.iter().enumerate() {
        if index != 0 {
            summary.push('\n');
        }
        let status = normalized_text::<32>(
            "planStatus",
            entry
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("pending"),
        )?;
        let content = normalized_text::<262144>(
            "planEntry",
            entry
                .get("content")
                .and_then(Value::as_str)
                .ok_or(TranslationError::MalformedFrame)?,
        )?;
        summary.push('[');
        summary.push_str(status.as_str());
        summary.push_str("] ");
        summary.push_str(content.as_str());
        if summary.len() > 262_144 {
            return Err(TranslationError::Limit("plan bytes"));
        }
    }
    Ok(AdapterEvent::Item {
        external_key: optional_item_id(update, "plan")?,
        lifecycle: ItemLifecycle::Streaming,
        tool_state: None,
        payload: NormalizedPayload::Plan {
            summary: SafeText::new("plan", summary)?,
        },
        update_mode: ItemUpdateMode::Replace,
    })
}

fn usage_event(update: &Map<String, Value>) -> Result<AdapterEvent, TranslationError> {
    let usage = update
        .get("usage")
        .and_then(Value::as_object)
        .unwrap_or(update);
    Ok(AdapterEvent::Item {
        external_key: optional_item_id(update, "usage")?,
        lifecycle: ItemLifecycle::Completed,
        tool_state: None,
        payload: NormalizedPayload::Usage {
            input_tokens: usage
                .get("inputTokens")
                .or_else(|| usage.get("input_tokens"))
                .and_then(Value::as_u64)
                .ok_or(TranslationError::MalformedFrame)?,
            output_tokens: usage
                .get("outputTokens")
                .or_else(|| usage.get("output_tokens"))
                .and_then(Value::as_u64)
                .ok_or(TranslationError::MalformedFrame)?,
        },
        update_mode: ItemUpdateMode::Replace,
    })
}

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
