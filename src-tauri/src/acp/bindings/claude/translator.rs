use std::collections::BTreeMap;

use serde_json::{Map, Value};

use crate::acp::{
    contract::{RequiredText, SafeText},
    intent::{
        ExternalId, NormalizedPayload, SafeResourceLocation, SafeResourceRef,
        UnavailableResourceReason,
    },
    lifecycle::{ItemLifecycle, ToolState, TurnState},
    limits::LIMITS,
};

use super::{sanitize::normalized_text, wire::decode_frame};

mod decode;
mod error;
mod permissions;

use decode::*;
pub(crate) use error::TranslationError;

pub(super) const MAX_EVENTS_PER_FRAME: usize = 16;
const MAX_TRACKED_TOOLS: usize = 512;
const MAX_TERMINAL_REQUESTS: usize = 16;

pub(crate) trait ResourceMapper {
    /// The absolute native path is transient and must not be retained in errors,
    /// events, SQLite, or WebView projections.
    fn map_absolute(
        &self,
        absolute_path: &str,
    ) -> Result<SafeResourceRef, UnavailableResourceReason>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PromptTerminal {
    Completed,
    Cancelled,
    Failed,
}

impl PromptTerminal {
    pub(crate) fn turn_state(self) -> TurnState {
        match self {
            Self::Completed => TurnState::Completed,
            Self::Cancelled => TurnState::Cancelled,
            Self::Failed => TurnState::Failed,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AdapterEvent {
    Item {
        external_key: ExternalId,
        lifecycle: ItemLifecycle,
        tool_state: Option<ToolState>,
        payload: NormalizedPayload,
    },
    PermissionDecline {
        request_id: ExternalId,
        tool_call_id: ExternalId,
    },
    TurnTerminal(PromptTerminal),
    IgnoredDuplicate,
    IgnoredLate,
}

pub(crate) struct ClaudeTranslator {
    external_session_id: ExternalId,
    tools: BTreeMap<String, ToolState>,
    text_snapshots: BTreeMap<String, String>,
    text_bytes: usize,
    permissions: BTreeMap<String, String>,
    resolved_permissions: BTreeMap<String, String>,
    pending_prompt: Option<String>,
    terminal_request_id: Option<String>,
    terminal_ledger: BTreeMap<String, PromptTerminal>,
    terminal: Option<PromptTerminal>,
}

impl ClaudeTranslator {
    pub(crate) fn new(external_session_id: impl Into<String>) -> Result<Self, TranslationError> {
        Ok(Self {
            external_session_id: RequiredText::new("externalSessionId", external_session_id)?,
            tools: BTreeMap::new(),
            text_snapshots: BTreeMap::new(),
            text_bytes: 0,
            permissions: BTreeMap::new(),
            resolved_permissions: BTreeMap::new(),
            pending_prompt: None,
            terminal_request_id: None,
            terminal_ledger: BTreeMap::new(),
            terminal: None,
        })
    }

    pub(crate) fn begin_prompt(&mut self, request_id: &str) -> Result<(), TranslationError> {
        let request_id = RequiredText::<512>::new("promptRequestId", request_id)?;
        if self.pending_prompt.is_some() {
            return Err(TranslationError::PromptAlreadyActive);
        }
        // ACP frames share one ordered stdout stream. Receiving the prior prompt
        // response establishes the boundary after all of its notifications; a
        // new prompt may therefore clear only the active terminal while keeping
        // a bounded terminal-request ledger for duplicate old responses.
        self.terminal = None;
        self.terminal_request_id = None;
        self.clear_turn_items();
        self.pending_prompt = Some(request_id.as_str().to_owned());
        Ok(())
    }

    pub(crate) fn translate_notification(
        &mut self,
        raw: &[u8],
        resources: &impl ResourceMapper,
    ) -> Result<Vec<AdapterEvent>, TranslationError> {
        let frame = decode_frame(raw)?;
        self.translate_notification_value(frame, resources)
    }

    pub(super) fn translate_notification_value(
        &mut self,
        frame: Value,
        resources: &impl ResourceMapper,
    ) -> Result<Vec<AdapterEvent>, TranslationError> {
        if self.terminal.is_some() {
            return Ok(vec![AdapterEvent::IgnoredLate]);
        }
        if frame.get("method").and_then(Value::as_str) != Some("session/update") {
            return Err(TranslationError::UnexpectedMethod);
        }
        let params = object(&frame, "params")?;
        self.validate_session(params.get("sessionId"))?;
        let update = params
            .get("update")
            .and_then(Value::as_object)
            .ok_or(TranslationError::MalformedFrame)?;
        let kind = string(update, "sessionUpdate")?;
        let events = match kind {
            "agent_message_chunk" => vec![self.text_event(update)?],
            "agent_thought_chunk" => vec![unsupported_event(
                "agent_thought_chunk",
                "Claude thought mapping is not proven by the pinned source packet",
            )?],
            "plan" => vec![plan_event(update)?],
            "tool_call" => vec![self.tool_call(update)?],
            "tool_call_update" => self.tool_update(update, resources)?,
            "usage" => vec![unsupported_event(
                "usage",
                "Claude usage mapping is not proven by the pinned source packet",
            )?],
            "claude_nested_transcript" => vec![unsupported_event(
                "claude_nested_transcript",
                "Nested Claude transcript metadata is display-only and not admitted",
            )?],
            other => vec![unsupported_event(
                other,
                "Unsupported bounded Claude ACP update",
            )?],
        };
        if events.len() > MAX_EVENTS_PER_FRAME {
            return Err(TranslationError::EventLimit);
        }
        Ok(events)
    }

    pub(crate) fn translate_prompt_result(
        &mut self,
        raw: &[u8],
    ) -> Result<Vec<AdapterEvent>, TranslationError> {
        let frame = decode_frame(raw)?;
        self.translate_prompt_result_value(frame)
    }

    pub(super) fn translate_prompt_result_value(
        &mut self,
        frame: Value,
    ) -> Result<Vec<AdapterEvent>, TranslationError> {
        let request_id = rpc_id(frame.get("id"))?;
        let error_message = frame
            .get("error")
            .and_then(Value::as_object)
            .and_then(|error| error.get("message"))
            .and_then(Value::as_str);
        let terminal = if frame.get("error").is_some() {
            PromptTerminal::Failed
        } else {
            match frame
                .get("result")
                .and_then(|result| result.get("stopReason"))
                .and_then(Value::as_str)
                .ok_or(TranslationError::MalformedFrame)?
            {
                "end_turn" => PromptTerminal::Completed,
                "cancelled" => PromptTerminal::Cancelled,
                "refusal" | "max_tokens" | "max_turn_requests" => PromptTerminal::Failed,
                _ => return Err(TranslationError::UnknownTerminal),
            }
        };
        if let Some(previous) = self.terminal_ledger.get(request_id.as_str()) {
            return if *previous == terminal {
                Ok(vec![AdapterEvent::IgnoredDuplicate])
            } else {
                Err(TranslationError::TerminalConflict)
            };
        }
        if let Some(current) = self.terminal {
            return if self.terminal_request_id.as_deref() == Some(request_id.as_str())
                && current == terminal
            {
                Ok(vec![AdapterEvent::IgnoredDuplicate])
            } else {
                Err(TranslationError::TerminalConflict)
            };
        }
        if self.pending_prompt.as_deref() != Some(request_id.as_str()) {
            return Err(TranslationError::ForeignId);
        }
        match self.terminal {
            None => {
                self.pending_prompt = None;
                self.terminal_request_id = Some(request_id.as_str().to_owned());
                self.terminal = Some(terminal);
                if self.terminal_ledger.len() >= MAX_TERMINAL_REQUESTS
                    && let Some(oldest) = self.terminal_ledger.keys().next().cloned()
                {
                    self.terminal_ledger.remove(&oldest);
                }
                self.terminal_ledger
                    .insert(request_id.as_str().to_owned(), terminal);
                let mut events = Vec::with_capacity(if error_message.is_some() { 2 } else { 1 });
                if let Some(message) = error_message {
                    events.push(AdapterEvent::Item {
                        external_key: RequiredText::new(
                            "externalItemId",
                            format!("error:{}", request_id.as_str()),
                        )?,
                        lifecycle: ItemLifecycle::Failed,
                        tool_state: None,
                        payload: NormalizedPayload::Error {
                            code: SafeText::new("errorCode", "claude_rpc_error")?,
                            message: normalized_text("errorMessage", message)?,
                            retryable: false,
                        },
                    });
                }
                events.push(AdapterEvent::TurnTerminal(terminal));
                self.clear_turn_items();
                Ok(events)
            }
            Some(current) if current == terminal => Ok(vec![AdapterEvent::IgnoredDuplicate]),
            Some(_) => Err(TranslationError::TerminalConflict),
        }
    }

    pub(super) fn reject_prompt(&mut self, request_id: &str) -> Result<(), TranslationError> {
        if self.pending_prompt.as_deref() != Some(request_id) || self.terminal.is_some() {
            return Err(TranslationError::ForeignId);
        }
        self.pending_prompt = None;
        self.clear_turn_items();
        Ok(())
    }

    fn validate_session(&self, value: Option<&Value>) -> Result<(), TranslationError> {
        let incoming = value
            .and_then(Value::as_str)
            .ok_or(TranslationError::MalformedFrame)?;
        let incoming = RequiredText::<512>::new("externalSessionId", incoming)?;
        if incoming != self.external_session_id {
            return Err(TranslationError::ForeignId);
        }
        Ok(())
    }

    fn text_event(
        &mut self,
        update: &Map<String, Value>,
    ) -> Result<AdapterEvent, TranslationError> {
        let content = update
            .get("content")
            .and_then(Value::as_object)
            .ok_or(TranslationError::MalformedFrame)?;
        if string(content, "type")? != "text" {
            return unsupported_event("agent_message_content", "Non-text content is not admitted");
        }
        let external_key = optional_external_key(update, "agent-message")?;
        let chunk = string(content, "text")?;
        let key = external_key.as_str().to_owned();
        if !self.text_snapshots.contains_key(&key)
            && self.text_snapshots.len().saturating_add(self.tools.len())
                >= LIMITS.wire.items_per_turn as usize
        {
            return Err(TranslationError::ItemLimit);
        }
        let current_snapshot = self.text_snapshots.get(&key);
        let current = current_snapshot.map_or(0, String::len);
        let item_bytes = current
            .checked_add(chunk.len())
            .filter(|bytes| *bytes <= LIMITS.wire.normalized_item_bytes as usize)
            .ok_or(TranslationError::ItemLimit)?;
        let aggregate = self
            .text_bytes
            .checked_sub(current)
            .and_then(|bytes| bytes.checked_add(item_bytes))
            .filter(|bytes| *bytes <= LIMITS.wire.item_bytes_per_turn as usize)
            .ok_or(TranslationError::ItemLimit)?;
        let mut snapshot = String::new();
        snapshot
            .try_reserve_exact(item_bytes)
            .map_err(|_| TranslationError::ItemLimit)?;
        if let Some(current) = current_snapshot {
            snapshot.push_str(current);
        }
        snapshot.push_str(chunk);
        // Sanitize the complete accumulated value, not each delta in
        // isolation: secrets and native paths may straddle frame boundaries.
        let safe_snapshot = normalized_text::<262144>("text", &snapshot)?;
        self.text_snapshots.insert(key, snapshot);
        self.text_bytes = aggregate;
        Ok(AdapterEvent::Item {
            external_key,
            lifecycle: ItemLifecycle::Streaming,
            tool_state: None,
            payload: NormalizedPayload::Text {
                text: safe_snapshot,
            },
        })
    }

    fn clear_turn_items(&mut self) {
        self.tools.clear();
        self.text_snapshots.clear();
        self.text_bytes = 0;
    }

    fn tool_call(&mut self, update: &Map<String, Value>) -> Result<AdapterEvent, TranslationError> {
        let id = RequiredText::new("toolCallId", string(update, "toolCallId")?)?;
        let category = tool_category(update.get("kind").and_then(Value::as_str));
        let state = parse_tool_state(update.get("status"), ToolState::Pending)?;
        if let Some(current) = self.tools.get(id.as_str()).copied() {
            if current == state {
                return Ok(AdapterEvent::IgnoredDuplicate);
            }
            return Err(TranslationError::DuplicateTool);
        }
        if self.tools.len() >= MAX_TRACKED_TOOLS
            || self.tools.len().saturating_add(self.text_snapshots.len())
                >= LIMITS.wire.items_per_turn as usize
        {
            return Err(TranslationError::ToolLimit);
        }
        self.tools.insert(id.as_str().to_owned(), state);
        Ok(AdapterEvent::Item {
            external_key: id,
            lifecycle: lifecycle_for_tool(state),
            tool_state: Some(state),
            payload: NormalizedPayload::ToolCall {
                category,
                label: SafeText::new("toolLabel", category_label(category))?,
                // Never retain rawInput, commands, paths, or adapter metadata.
                input_summary: None,
            },
        })
    }

    fn tool_update(
        &mut self,
        update: &Map<String, Value>,
        resources: &impl ResourceMapper,
    ) -> Result<Vec<AdapterEvent>, TranslationError> {
        let id = RequiredText::new("toolCallId", string(update, "toolCallId")?)?;
        let current = self
            .tools
            .get(id.as_str())
            .copied()
            .ok_or(TranslationError::OrphanToolUpdate)?;
        let next = parse_tool_state(update.get("status"), current)?;
        if next == current {
            return Ok(vec![AdapterEvent::IgnoredDuplicate]);
        }
        transition_tool(current, next)?;
        self.tools.insert(id.as_str().to_owned(), next);
        let category = tool_category(update.get("kind").and_then(Value::as_str));
        let mut events = vec![AdapterEvent::Item {
            external_key: id.clone(),
            lifecycle: lifecycle_for_tool(next),
            tool_state: Some(next),
            payload: NormalizedPayload::ToolUpdateResult {
                category,
                label: SafeText::new("toolLabel", category_label(category))?,
                // Never retain rawOutput or terminal content.
                output_summary: None,
            },
        }];
        if let Some(content) = update.get("content").and_then(Value::as_array) {
            if content.len() >= MAX_EVENTS_PER_FRAME {
                return Err(TranslationError::EventLimit);
            }
            for (index, entry) in content.iter().enumerate() {
                if entry.get("type").and_then(Value::as_str) == Some("diff") {
                    let path = entry
                        .get("path")
                        .and_then(Value::as_str)
                        .ok_or(TranslationError::MalformedFrame)?;
                    let resource = match resources.map_absolute(path) {
                        Ok(resource) => SafeResourceLocation::Resource { resource },
                        Err(reason) => SafeResourceLocation::unavailable(reason),
                    };
                    let old_text = entry.get("oldText").and_then(Value::as_str).unwrap_or("");
                    let new_text = entry.get("newText").and_then(Value::as_str).unwrap_or("");
                    events.push(AdapterEvent::Item {
                        external_key: RequiredText::new(
                            "externalItemId",
                            format!("{}:diff:{index}", id.as_str()),
                        )?,
                        lifecycle: ItemLifecycle::Completed,
                        tool_state: None,
                        payload: NormalizedPayload::FileChangeDiff {
                            resource,
                            summary: SafeText::new(
                                "fileSummary",
                                "File change reported by Claude ACP",
                            )?,
                            additions: Some(line_count(new_text)),
                            deletions: Some(line_count(old_text)),
                        },
                    });
                }
            }
        }
        Ok(events)
    }
}
