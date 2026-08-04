use serde_json::{Map, Value};

use crate::acp::{
    contract::{BoundedVec, SafeText},
    intent::{NormalizedPayload, ToolCategory},
    lifecycle::{ItemLifecycle, ToolState},
    limits::LIMITS,
};

use super::{
    IngressError, ResourceMapper,
    support::{content, safe_text, text, unsupported},
};

type ItemNormalization = (NormalizedPayload, Option<ToolState>, Option<ItemLifecycle>);

pub(super) fn normalize_item<M: ResourceMapper>(
    item: &Map<String, Value>,
    lifecycle: ItemLifecycle,
    mapper: &M,
) -> Result<ItemNormalization, IngressError> {
    let kind = text(item, "type", 128)?;
    match kind {
        "agentMessage" => Ok((
            NormalizedPayload::Text {
                text: safe_text(item.get("text"), 262_144, "text")?,
            },
            None,
            None,
        )),
        "plan" => Ok((
            NormalizedPayload::Plan {
                summary: safe_text(item.get("text"), 262_144, "plan")?,
            },
            None,
            None,
        )),
        "reasoning" => Ok((
            NormalizedPayload::ThoughtSummary {
                summary: SafeText::new("summary", "Reasoning summary available")
                    .map_err(content)?,
            },
            None,
            None,
        )),
        "commandExecution" => tool(item, lifecycle, ToolCategory::Command, "Command"),
        "mcpToolCall" => tool(item, lifecycle, ToolCategory::Mcp, "MCP tool"),
        "dynamicToolCall" => tool(item, lifecycle, ToolCategory::Other, "Tool"),
        "webSearch" => tool(item, lifecycle, ToolCategory::Web, "Web search"),
        "collabAgentToolCall" | "subAgentActivity" => Ok((
            NormalizedPayload::Collaboration {
                label: SafeText::new("activityLabel", "Harness-owned child activity")
                    .map_err(content)?,
                message: None,
                receiver_labels: BoundedVec::new("receivers", vec![]).map_err(content)?,
            },
            None,
            None,
        )),
        "fileChange" => file_change(item, mapper),
        other => Ok((
            unsupported(other, "Unsupported Codex item")?,
            None,
            Some(ItemLifecycle::Unsupported),
        )),
    }
}

fn tool(
    item: &Map<String, Value>,
    lifecycle: ItemLifecycle,
    category: ToolCategory,
    label: &str,
) -> Result<ItemNormalization, IngressError> {
    let status = item
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("inProgress");
    let tool_state = match status {
        "inProgress" => ToolState::InProgress,
        "completed" => ToolState::Completed,
        "failed" => ToolState::Failed,
        "declined" => ToolState::Declined,
        _ => return Err(IngressError::Malformed),
    };
    let terminal = match tool_state {
        ToolState::Completed => Some(ItemLifecycle::Completed),
        ToolState::Failed => Some(ItemLifecycle::Failed),
        ToolState::Declined => Some(ItemLifecycle::Cancelled),
        _ => None,
    };
    let label = SafeText::new("toolLabel", label).map_err(content)?;
    let payload = if lifecycle == ItemLifecycle::Started {
        NormalizedPayload::ToolCall {
            category,
            label,
            input_summary: None,
        }
    } else {
        NormalizedPayload::ToolUpdateResult {
            category,
            label,
            output_summary: None,
        }
    };
    Ok((payload, Some(tool_state), terminal))
}

fn file_change<M: ResourceMapper>(
    item: &Map<String, Value>,
    mapper: &M,
) -> Result<ItemNormalization, IngressError> {
    let changes = item
        .get("changes")
        .and_then(Value::as_array)
        .ok_or(IngressError::Malformed)?;
    if changes.len() != 1 {
        return Ok((
            unsupported("fileChange", "File change spans multiple resources")?,
            None,
            Some(ItemLifecycle::Unsupported),
        ));
    }
    let change = changes[0].as_object().ok_or(IngressError::Malformed)?;
    let absolute = text(
        change,
        "path",
        LIMITS.wire.absolute_path_bytes_native_only as usize,
    )?;
    let resource = match mapper.map_existing(absolute) {
        Ok(resource) => crate::acp::intent::SafeResourceLocation::Resource { resource },
        Err(reason) => crate::acp::intent::SafeResourceLocation::unavailable(reason),
    };
    Ok((
        NormalizedPayload::FileChangeDiff {
            resource,
            summary: SafeText::new("fileSummary", "Codex reported a file change")
                .map_err(content)?,
            additions: None,
            deletions: None,
        },
        Some(ToolState::Completed),
        Some(ItemLifecycle::Completed),
    ))
}
