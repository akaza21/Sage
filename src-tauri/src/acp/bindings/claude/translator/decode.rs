use serde_json::{Map, Value};

use crate::acp::{
    contract::{RequiredText, SafeText},
    intent::{ExternalId, NormalizedPayload, ToolCategory},
    lifecycle::{ItemLifecycle, ToolState},
};

use super::super::sanitize::normalized_text;
use super::{AdapterEvent, TranslationError};

const MAX_PLAN_ENTRIES: usize = 128;

pub(super) fn plan_event(update: &Map<String, Value>) -> Result<AdapterEvent, TranslationError> {
    let entries = update
        .get("entries")
        .and_then(Value::as_array)
        .ok_or(TranslationError::MalformedFrame)?;
    if entries.len() > MAX_PLAN_ENTRIES {
        return Err(TranslationError::PlanLimit);
    }
    let mut summary = String::new();
    for (index, entry) in entries.iter().enumerate() {
        let content = entry
            .get("content")
            .and_then(Value::as_str)
            .ok_or(TranslationError::MalformedFrame)?;
        let status = entry
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("pending");
        if index != 0 {
            summary.push('\n');
        }
        summary.push('[');
        summary.push_str(normalized_text::<32>("planStatus", status)?.as_str());
        summary.push_str("] ");
        summary.push_str(normalized_text::<262144>("planEntry", content)?.as_str());
        if summary.len() > 262_144 {
            return Err(TranslationError::PlanLimit);
        }
    }
    Ok(AdapterEvent::Item {
        external_key: optional_external_key(update, "plan")?,
        lifecycle: ItemLifecycle::Streaming,
        tool_state: None,
        payload: NormalizedPayload::Plan {
            summary: SafeText::new("plan", summary)?,
        },
    })
}

pub(super) fn unsupported_event(
    kind: &str,
    summary: &str,
) -> Result<AdapterEvent, TranslationError> {
    Ok(AdapterEvent::Item {
        external_key: RequiredText::new("externalItemId", format!("unsupported:{kind}"))?,
        lifecycle: ItemLifecycle::Unsupported,
        tool_state: None,
        payload: NormalizedPayload::Unsupported {
            external_kind: SafeText::new("externalKind", kind)?,
            summary: SafeText::new("unsupportedSummary", summary)?,
        },
    })
}

pub(super) fn optional_external_key(
    object: &Map<String, Value>,
    fallback: &str,
) -> Result<ExternalId, TranslationError> {
    RequiredText::new(
        "externalItemId",
        object
            .get("itemId")
            .and_then(Value::as_str)
            .unwrap_or(fallback),
    )
    .map_err(Into::into)
}

pub(super) fn tool_category(kind: Option<&str>) -> ToolCategory {
    match kind {
        Some("read" | "edit" | "delete" | "move") => ToolCategory::File,
        Some("search") => ToolCategory::Search,
        Some("fetch") => ToolCategory::Web,
        Some("execute") => ToolCategory::Command,
        _ => ToolCategory::Other,
    }
}

pub(super) fn category_label(category: ToolCategory) -> &'static str {
    match category {
        ToolCategory::File => "File tool",
        ToolCategory::Diff => "Diff tool",
        ToolCategory::Command => "Command tool",
        ToolCategory::Search => "Search tool",
        ToolCategory::Web => "Web tool",
        ToolCategory::Mcp => "MCP tool",
        ToolCategory::Subagent => "Nested activity",
        ToolCategory::Other => "Claude tool",
    }
}

pub(super) fn parse_tool_state(
    value: Option<&Value>,
    default: ToolState,
) -> Result<ToolState, TranslationError> {
    match value.and_then(Value::as_str) {
        None => Ok(default),
        Some("pending") => Ok(ToolState::Pending),
        Some("in_progress") => Ok(ToolState::InProgress),
        Some("completed") => Ok(ToolState::Completed),
        Some("failed") => Ok(ToolState::Failed),
        _ => Err(TranslationError::UnknownToolState),
    }
}

pub(super) fn transition_tool(current: ToolState, next: ToolState) -> Result<(), TranslationError> {
    if current.can_transition_to(next) {
        Ok(())
    } else if current.is_terminal() {
        Err(TranslationError::TerminalConflict)
    } else {
        Err(TranslationError::InvalidToolTransition)
    }
}

pub(super) fn lifecycle_for_tool(state: ToolState) -> ItemLifecycle {
    match state {
        ToolState::Pending => ItemLifecycle::Started,
        ToolState::InProgress | ToolState::WaitingForApproval => ItemLifecycle::Streaming,
        ToolState::Completed | ToolState::Declined => ItemLifecycle::Completed,
        ToolState::Failed => ItemLifecycle::Failed,
        ToolState::Cancelled => ItemLifecycle::Cancelled,
    }
}

pub(super) fn rpc_id(value: Option<&Value>) -> Result<ExternalId, TranslationError> {
    let value = match value {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Number(value)) => value.to_string(),
        _ => return Err(TranslationError::MalformedFrame),
    };
    RequiredText::new("requestId", value).map_err(Into::into)
}

pub(super) fn object<'a>(
    parent: &'a Value,
    key: &str,
) -> Result<&'a Map<String, Value>, TranslationError> {
    parent
        .get(key)
        .and_then(Value::as_object)
        .ok_or(TranslationError::MalformedFrame)
}

pub(super) fn object_value(value: Option<&Value>) -> Result<&Map<String, Value>, TranslationError> {
    value
        .and_then(Value::as_object)
        .ok_or(TranslationError::MalformedFrame)
}

pub(super) fn string<'a>(
    object: &'a Map<String, Value>,
    key: &str,
) -> Result<&'a str, TranslationError> {
    object
        .get(key)
        .and_then(Value::as_str)
        .ok_or(TranslationError::MalformedFrame)
}

pub(super) fn line_count(value: &str) -> u64 {
    if value.is_empty() {
        0
    } else {
        value.lines().count() as u64
    }
}
