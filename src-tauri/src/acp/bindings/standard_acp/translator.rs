use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};

use crate::acp::{
    contract::{BoundedVec, RequiredText, SafeText},
    intent::{
        COMMAND_RESOURCE_LIMIT, ExternalId, NormalizedPayload, SafeResourceLocation,
        SafeResourceRef, ToolCategory, UnavailableResourceReason,
    },
    lifecycle::{ItemLifecycle, ToolState},
    limits::LIMITS,
};

#[cfg(test)]
use super::wire::decode_frame;
use super::{
    profile::StandardAcpProfile,
    sanitize::normalized_text,
    surface::{SessionUpdateKind, UNKNOWN_NOTIFICATION_REASON, UnmappedUpdate, unmapped_update},
};

mod decode;
mod diff_content;
mod diff_records;
mod error;
mod images;
mod permissions;
mod terminal;
mod tool_execution;
mod tool_input;
mod tool_snapshots;
mod tools;

use decode::{
    object, optional_external_key, plan_event, stated_context_window, string, unsupported_event,
    usage_event, usage_object,
};
pub(crate) use error::TranslationError;
pub(crate) use terminal::PromptTerminal;

pub(super) const MAX_EVENTS_PER_FRAME: usize = 16;
const MAX_TRACKED_TOOLS: usize = 512;

pub(crate) trait ResourceMapper {
    /// The absolute native path is transient and must not be retained in errors,
    /// events, SQLite, or WebView projections.
    fn map_absolute(
        &self,
        absolute_path: &str,
    ) -> Result<SafeResourceRef, UnavailableResourceReason>;

    fn map_missing_display(&self, _absolute_path: &str) -> Option<SafeResourceRef> {
        None
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
    /// Bounded old/new bytes for the preceding `FileChangeDiff` item with the
    /// same external key. Stored as BLOBs; never serialized into the payload.
    FilePair {
        external_key: ExternalId,
        old_text: String,
        new_text: String,
    },
    #[allow(
        dead_code,
        reason = "consumers handle this variant; nothing produces it yet"
    )]
    PermissionDecline {
        request_id: ExternalId,
        tool_call_id: ExternalId,
    },
    Usage(NormalizedPayload),
    TurnRunning,
    TurnTerminal(PromptTerminal),
    Absorbed,
    IgnoredDuplicate,
    IgnoredLate,
}

pub(crate) struct StandardAcpTranslator {
    profile: &'static StandardAcpProfile,
    external_session_id: ExternalId,
    tools: BTreeMap<String, ToolState>,
    tool_categories: BTreeMap<String, ToolCategory>,
    tool_labels: BTreeMap<String, SafeText<512>>,
    tool_inputs: BTreeMap<String, SafeText<65536>>,
    diff_records: BTreeMap<String, diff_records::DiffRecord>,
    command_snapshots: BTreeMap<
        String,
        (
            Option<SafeText<65536>>,
            BoundedVec<SafeResourceLocation, COMMAND_RESOURCE_LIMIT>,
        ),
    >,
    command_snapshot_bytes: usize,
    text_snapshots: BTreeMap<String, String>,
    image_keys: BTreeSet<String>,
    text_bytes: usize,
    permissions: BTreeMap<String, String>,
    resolved_permissions: BTreeMap<String, String>,
    settled_reports: BTreeMap<String, ToolState>,
    carry: Vec<AdapterEvent>,
    pending_prompt: Option<String>,
    running_emitted: bool,
    context_window: Option<u32>,
    terminal_request_id: Option<String>,
    terminal_ledger: BTreeMap<String, PromptTerminal>,
    terminal: Option<PromptTerminal>,
}

impl StandardAcpTranslator {
    pub(crate) fn is_sole_pending_tool(&self, tool_id: &str) -> bool {
        self.tools
            .iter()
            .filter(|(_, state)| !state.is_terminal())
            .count()
            == 1
            && self
                .tools
                .get(tool_id)
                .is_some_and(|state| !state.is_terminal())
    }

    pub(crate) fn new(
        profile: &'static StandardAcpProfile,
        external_session_id: impl Into<String>,
    ) -> Result<Self, TranslationError> {
        Ok(Self {
            profile,
            external_session_id: RequiredText::new("externalSessionId", external_session_id)?,
            tools: BTreeMap::new(),
            tool_categories: BTreeMap::new(),
            tool_labels: BTreeMap::new(),
            tool_inputs: BTreeMap::new(),
            diff_records: BTreeMap::new(),
            command_snapshots: BTreeMap::new(),
            command_snapshot_bytes: 0,
            text_snapshots: BTreeMap::new(),
            image_keys: BTreeSet::new(),
            text_bytes: 0,
            permissions: BTreeMap::new(),
            resolved_permissions: BTreeMap::new(),
            settled_reports: BTreeMap::new(),
            carry: Vec::new(),
            pending_prompt: None,
            running_emitted: false,
            context_window: None,
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
        self.running_emitted = false;
        self.context_window = None;
        self.clear_turn_items();
        self.pending_prompt = Some(request_id.as_str().to_owned());
        Ok(())
    }

    /// Releases concurrency capacity after the runtime wrote an allow reply.
    /// The runtime owns bounded reply replay; the tool still settles on ingress.
    pub(crate) fn permission_allow_written(
        &mut self,
        request_id: &str,
    ) -> Result<(), TranslationError> {
        self.permissions
            .remove(request_id)
            .ok_or(TranslationError::ForeignId)?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn translate_notification(
        &mut self,
        raw: &[u8],
        resources: &impl ResourceMapper,
    ) -> Result<Vec<AdapterEvent>, TranslationError> {
        let frame = decode_frame(raw)?;
        self.translate_notification_value(frame, resources)
    }

    pub(crate) fn translate_notification_value(
        &mut self,
        frame: Value,
        resources: &impl ResourceMapper,
    ) -> Result<Vec<AdapterEvent>, TranslationError> {
        if self.terminal.is_some() {
            return Ok(vec![AdapterEvent::IgnoredLate]);
        }
        let method = frame
            .get("method")
            .and_then(Value::as_str)
            .ok_or(TranslationError::UnexpectedMethod)?;
        if method != "session/update" {
            if frame.get("id").is_some() {
                return Err(TranslationError::UnexpectedMethod);
            }
            return Ok(vec![self.unsupported(method, UNKNOWN_NOTIFICATION_REASON)?]);
        }
        let params = object(&frame, "params")?;
        self.validate_session(params.get("sessionId"))?;
        let update = params
            .get("update")
            .and_then(Value::as_object)
            .ok_or(TranslationError::MalformedFrame)?;
        let kind = string(update, "sessionUpdate")?;
        let mut events = match SessionUpdateKind::parse(kind) {
            Some(SessionUpdateKind::AgentMessageChunk) => vec![self.content_event(update, false)?],
            Some(SessionUpdateKind::AgentThoughtChunk) => vec![self.content_event(update, true)?],
            Some(SessionUpdateKind::Plan) => {
                let external_key = self.external_key(update, "plan")?;
                vec![plan_event(update, external_key)?]
            }
            Some(SessionUpdateKind::ToolCall) => self.tool_call(update, resources)?,
            Some(SessionUpdateKind::ToolCallUpdate) => self.tool_update(update, resources)?,
            Some(SessionUpdateKind::UsageUpdate) => vec![self.usage_update(update)],
            _ => match unmapped_update(self.profile, kind) {
                UnmappedUpdate::Absorbed => vec![AdapterEvent::Absorbed],
                UnmappedUpdate::Surfaced(reason) => vec![self.unsupported(kind, reason)?],
            },
        };
        if let Some(running) = self.turn_running() {
            events.insert(0, running);
        }
        if events.len() > MAX_EVENTS_PER_FRAME {
            return Err(TranslationError::EventLimit);
        }
        Ok(events)
    }

    fn usage_update(&mut self, update: &Map<String, Value>) -> AdapterEvent {
        let usage = usage_object(update);
        if let Some(window) = stated_context_window(usage) {
            self.context_window = Some(window);
        }
        usage_event(usage, self.context_window).unwrap_or(AdapterEvent::Absorbed)
    }

    pub(super) fn turn_running(&mut self) -> Option<AdapterEvent> {
        if self.running_emitted || self.pending_prompt.is_none() {
            return None;
        }
        self.running_emitted = true;
        Some(AdapterEvent::TurnRunning)
    }

    pub(super) fn turn_scope(&self) -> Option<&str> {
        self.pending_prompt.as_deref()
    }

    pub(super) fn external_key(
        &self,
        update: &Map<String, Value>,
        fallback: &str,
    ) -> Result<ExternalId, TranslationError> {
        optional_external_key(update, fallback, self.turn_scope())
    }

    pub(super) fn unsupported(
        &self,
        kind: &str,
        summary: &str,
    ) -> Result<AdapterEvent, TranslationError> {
        unsupported_event(kind, summary, self.turn_scope())
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

    fn content_event(
        &mut self,
        update: &Map<String, Value>,
        thought: bool,
    ) -> Result<AdapterEvent, TranslationError> {
        let content = update
            .get("content")
            .and_then(Value::as_object)
            .ok_or(TranslationError::MalformedFrame)?;
        match string(content, "type")? {
            "text" => self.text_event(update, content, thought),
            "image" if !thought => self.image_event(update, content),
            _ => self.unsupported("agent_message_content", "Non-text content is not admitted"),
        }
    }

    fn text_event(
        &mut self,
        update: &Map<String, Value>,
        content: &Map<String, Value>,
        thought: bool,
    ) -> Result<AdapterEvent, TranslationError> {
        let external_key =
            self.external_key(update, if thought { "thought" } else { "agent-message" })?;
        let chunk = string(content, "text")?;
        let key = external_key.as_str().to_owned();
        if !self.text_snapshots.contains_key(&key)
            && self.tracked_item_count() >= LIMITS.wire.items_per_turn as usize
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
            payload: if thought {
                NormalizedPayload::ThoughtSummary {
                    summary: safe_snapshot,
                }
            } else {
                NormalizedPayload::Text {
                    text: safe_snapshot,
                }
            },
        })
    }

    pub(super) fn tracked_item_count(&self) -> usize {
        self.text_snapshots
            .len()
            .saturating_add(self.tools.len())
            .saturating_add(self.image_keys.len())
    }

    fn clear_turn_items(&mut self) {
        self.tools.clear();
        self.tool_categories.clear();
        self.tool_labels.clear();
        self.tool_inputs.clear();
        self.diff_records.clear();
        self.settled_reports.clear();
        self.carry.clear();
        self.command_snapshots.clear();
        self.command_snapshot_bytes = 0;
        self.text_snapshots.clear();
        self.image_keys.clear();
        self.text_bytes = 0;
    }
}
