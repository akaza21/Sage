use serde_json::{Map, Value};

use crate::acp::{
    contract::{ItemLifecycle, RequiredText, ToolState},
    intent::{ExternalId, ToolCategory},
};

use super::super::TranslationError;
use crate::acp::bindings::omp::wire::opaque_id;

pub(crate) fn tool_category(update: &Map<String, Value>) -> ToolCategory {
    let kind = update.get("kind").and_then(Value::as_str).unwrap_or("");
    let name = update
        .get("name")
        .or_else(|| update.get("title"))
        .and_then(Value::as_str)
        .filter(|name| name.len() <= 512)
        .unwrap_or("")
        .to_ascii_lowercase();
    match kind {
        "read" | "edit" | "delete" | "move" => ToolCategory::Read,
        "search" => ToolCategory::Search,
        "fetch" => ToolCategory::Fetch,
        "execute" => ToolCategory::Execute,
        _ if name.contains("diff") || name.contains("patch") => ToolCategory::Edit,
        _ if name.contains("search") || name.contains("grep") || name.contains("find") => {
            ToolCategory::Search
        }
        _ if name.contains("browser") || name.contains("web") || name.contains("fetch") => {
            ToolCategory::Fetch
        }
        _ if name.contains("mcp") => ToolCategory::Mcp,
        _ if name.contains("subagent") || name.contains("sub-agent") || name == "task" => {
            ToolCategory::Subagent
        }
        _ if name.contains("shell")
            || name.contains("powershell")
            || name.contains("bash")
            || name.contains("process")
            || name.contains("lsp")
            || name.contains("dap") =>
        {
            ToolCategory::Execute
        }
        _ => ToolCategory::Other,
    }
}

pub(crate) fn category_label(category: ToolCategory) -> &'static str {
    match category {
        ToolCategory::Read => "File tool",
        ToolCategory::Edit => "Diff tool",
        ToolCategory::Delete => "Delete tool",
        ToolCategory::Move => "Move tool",
        ToolCategory::Execute => "Command tool",
        ToolCategory::Search => "Search tool",
        ToolCategory::Fetch => "Web tool",
        ToolCategory::Think => "Thinking tool",
        ToolCategory::SwitchMode => "Mode switch",
        ToolCategory::Mcp => "MCP tool",
        ToolCategory::Subagent => "Subagent activity",
        ToolCategory::Other => "OMP tool",
    }
}

pub(super) fn lifecycle(state: ToolState) -> ItemLifecycle {
    match state {
        ToolState::Pending => ItemLifecycle::Started,
        ToolState::InProgress | ToolState::WaitingForApproval => ItemLifecycle::Streaming,
        ToolState::Completed | ToolState::Declined => ItemLifecycle::Completed,
        ToolState::Failed => ItemLifecycle::Failed,
        ToolState::Cancelled => ItemLifecycle::Cancelled,
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
        Some("waiting_for_approval") => Ok(ToolState::WaitingForApproval),
        Some("completed") => Ok(ToolState::Completed),
        Some("failed") => Ok(ToolState::Failed),
        Some("declined") => Ok(ToolState::Declined),
        Some("cancelled") => Ok(ToolState::Cancelled),
        Some(_) => Err(TranslationError::ToolTransition),
    }
}

pub(crate) fn external_id(value: Option<&Value>) -> Result<ExternalId, TranslationError> {
    RequiredText::new("externalId", opaque_id(value)?).map_err(Into::into)
}

pub(super) fn optional_item_id(
    update: &Map<String, Value>,
    fallback: &str,
) -> Result<ExternalId, TranslationError> {
    match update.get("itemId") {
        Some(value) => external_id(Some(value)),
        None => RequiredText::new("externalItemId", fallback).map_err(Into::into),
    }
}

pub(crate) fn object(value: Option<&Value>) -> Result<&Map<String, Value>, TranslationError> {
    value
        .and_then(Value::as_object)
        .ok_or(TranslationError::MalformedFrame)
}

pub(super) fn required_string<'a>(
    object: &'a Map<String, Value>,
    key: &str,
) -> Result<&'a str, TranslationError> {
    object
        .get(key)
        .and_then(Value::as_str)
        .ok_or(TranslationError::MalformedFrame)
}

pub(super) fn line_count(value: Option<&str>) -> Option<u64> {
    value.map(|text| {
        if text.is_empty() {
            0
        } else {
            text.lines().count() as u64
        }
    })
}
