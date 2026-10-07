use serde_json::{Map, Value};

use crate::acp::{
    command_output::retain_command_output,
    contract::{BoundedVec, SafeText},
    declared_test::CommandActivity,
    image_admit::{ImageAdmitError, admit_image_data_url},
    intent::{NormalizedPayload, ToolCategory, ToolExecution},
    lifecycle::{ItemLifecycle, ToolState},
    limits::LIMITS,
    runtime::host_browse::{CURATED_BROWSE_TOOL_NAME, admit_browse_url},
};

use super::super::{CODEX_BROWSE_TOOL_NAME, state::MaterializedCommandItem};
use super::{
    IngressError, ResourceMapper,
    support::{
        command_cwd, command_exit_code, content, is_browse_alias, safe_text, sanitize,
        sanitized_command_line, sanitized_command_output, text, unsupported,
    },
};

type ItemNormalization = (NormalizedPayload, Option<ToolState>, Option<ItemLifecycle>);

mod presentation;

use presentation::{
    ToolPresentationInput, bounded_label, bounded_summary, context_compaction, declared_tool_state,
    lenient_tool_state, mcp_tool_label, parse_tool_state, tool, tool_with_summary,
    tool_with_summary_status,
};

pub(super) fn normalize_materialized_command(
    command: &MaterializedCommandItem,
) -> Result<ItemNormalization, IngressError> {
    let (payload, tool_state, terminal) = tool_with_summary_status(ToolPresentationInput {
        category: ToolCategory::Execute,
        label: &bounded_label(command.command.as_deref(), "Command"),
        tool_state: parse_tool_state(command.status.as_str()).ok_or(IngressError::Malformed)?,
        input_summary: command.command.clone(),
        output_summary: command.output.clone(),
        locations: command.locations.clone(),
        command_activity: command.activity,
        execution: ToolExecution {
            cwd: command.cwd.clone(),
            exit_code: command.exit_code,
            ..Default::default()
        },
    })?;
    Ok((payload, tool_state, terminal))
}

pub(super) fn normalize_item<M: ResourceMapper>(
    item: &Map<String, Value>,
    lifecycle: ItemLifecycle,
    mapper: &M,
) -> Result<Option<ItemNormalization>, IngressError> {
    let kind = text(item, "type", 128)?;
    match kind {
        // Sage persists the client prompt before dispatching `turn/start`.
        // Codex subsequently echoes that same user-facing item, so retaining
        // it again would duplicate the user bubble. Keep the echo out of the
        // durable projection without treating a known protocol item as an
        // unsupported activity.
        "userMessage" => Ok(None),
        "agentMessage" => Ok(Some((
            NormalizedPayload::Text {
                text: safe_text(item.get("text"), 262_144, "text")?,
            },
            None,
            None,
        ))),
        "plan" => Ok(Some((
            NormalizedPayload::Plan {
                summary: safe_text(item.get("text"), 262_144, "plan")?,
            },
            None,
            None,
        ))),
        "reasoning" => reasoning(item),
        "commandExecution" => command_execution(item, mapper).map(Some),
        "mcpToolCall" => tool(item, ToolCategory::Mcp, &mcp_tool_label(item)).map(Some),
        "contextCompaction" => context_compaction(item, lifecycle).map(Some),
        "dynamicToolCall" => dynamic_tool(item).map(Some),
        "webSearch" => tool(item, ToolCategory::Fetch, "Web search").map(Some),
        "collabAgentToolCall" | "subAgentActivity" => presentation::collaboration(item).map(Some),
        "fileChange" if !lifecycle.is_terminal() => Ok(None),
        "fileChange" => file_change(item, mapper).map(Some),
        "imageGeneration" => image_generation(item, lifecycle),
        "imageView" => image_view(item, mapper, lifecycle).map(Some),
        "enteredReviewMode" => review_mode(item, "Entered review mode", lifecycle).map(Some),
        "exitedReviewMode" => review_mode(item, "Exited review mode", lifecycle).map(Some),
        other => other_item(item, other, lifecycle).map(Some),
    }
}

fn image_view<M: ResourceMapper>(
    item: &Map<String, Value>,
    mapper: &M,
    lifecycle: ItemLifecycle,
) -> Result<ItemNormalization, IngressError> {
    let locations = item
        .get("path")
        .and_then(Value::as_str)
        .filter(|path| is_well_formed_read_path(path))
        .map(|absolute| mapped_location(absolute, mapper))
        .map(|location| BoundedVec::new("imageViewLocations", vec![location]))
        .transpose()
        .map_err(content)?
        .unwrap_or_default();
    tool_with_summary_status(ToolPresentationInput {
        category: ToolCategory::Read,
        label: "Image view",
        tool_state: lenient_tool_state(item, lifecycle),
        input_summary: None,
        output_summary: None,
        locations,
        command_activity: CommandActivity::Ordinary,
        execution: ToolExecution::default(),
    })
}

fn review_mode(
    item: &Map<String, Value>,
    label: &str,
    lifecycle: ItemLifecycle,
) -> Result<ItemNormalization, IngressError> {
    tool_with_summary_status(ToolPresentationInput {
        category: ToolCategory::SwitchMode,
        label,
        tool_state: lenient_tool_state(item, lifecycle),
        input_summary: item
            .get("review")
            .and_then(Value::as_str)
            .and_then(bounded_summary),
        output_summary: None,
        locations: BoundedVec::default(),
        command_activity: CommandActivity::Ordinary,
        execution: ToolExecution::default(),
    })
}

fn other_item(
    item: &Map<String, Value>,
    kind: &str,
    lifecycle: ItemLifecycle,
) -> Result<ItemNormalization, IngressError> {
    let title = ["title", "name", "tool"]
        .into_iter()
        .find_map(|field| item.get(field).and_then(Value::as_str))
        .map(sanitize);
    tool_with_summary_status(ToolPresentationInput {
        category: ToolCategory::Other,
        label: &bounded_label(title.as_deref(), &sanitize(kind)),
        tool_state: lenient_tool_state(item, lifecycle),
        input_summary: serde_json::to_string(item)
            .ok()
            .as_deref()
            .and_then(bounded_summary),
        output_summary: None,
        locations: BoundedVec::default(),
        command_activity: CommandActivity::Ordinary,
        execution: ToolExecution::default(),
    })
}

fn reasoning(item: &Map<String, Value>) -> Result<Option<ItemNormalization>, IngressError> {
    let parts = ["summary", "content"]
        .into_iter()
        .filter_map(|field| item.get(field).and_then(Value::as_array))
        .flatten()
        .map(|entry| entry.as_str().ok_or(IngressError::Malformed))
        .collect::<Result<Vec<_>, IngressError>>()?;
    let joined = parts.join("\n\n");
    if joined.is_empty() {
        return Ok(None);
    }
    if joined.len() > 262_144 {
        return Err(IngressError::Content);
    }
    Ok(Some((
        NormalizedPayload::ThoughtSummary {
            summary: SafeText::new("summary", sanitize(&joined)).map_err(content)?,
        },
        None,
        None,
    )))
}

fn dynamic_tool(item: &Map<String, Value>) -> Result<ItemNormalization, IngressError> {
    let label = text(item, "tool", 128)?;
    if label == CODEX_BROWSE_TOOL_NAME {
        let href = item
            .get("arguments")
            .and_then(Value::as_object)
            .and_then(|arguments| arguments.get("url"))
            .and_then(Value::as_str)
            .and_then(admit_browse_url)
            .ok_or(IngressError::Malformed)?;
        return tool_with_summary(
            item,
            ToolCategory::Fetch,
            CURATED_BROWSE_TOOL_NAME,
            Some(href),
            None,
            Default::default(),
        );
    }
    if is_browse_alias(label) {
        return Err(IngressError::UnsupportedServerRequest);
    }
    tool(item, ToolCategory::Other, "Tool")
}

fn command_execution<M: ResourceMapper>(
    item: &Map<String, Value>,
    mapper: &M,
) -> Result<ItemNormalization, IngressError> {
    let command = item
        .get("command")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && !value.contains('\0'))
        .map(sanitized_command_line)
        .transpose()?
        .filter(|value| !value.is_empty());
    let output = item
        .get("aggregatedOutput")
        .and_then(Value::as_str)
        .map(|value| {
            sanitized_command_output(value).and_then(|value| {
                retain_command_output(&value)
                    .and_then(|retained| retained.display_text())
                    .ok_or(IngressError::Content)
            })
        })
        .transpose()?;
    let locations = item
        .get("commandActions")
        .and_then(Value::as_array)
        .filter(|actions| actions.len() == 1)
        .and_then(|actions| actions[0].as_object())
        .filter(|action| action.get("type").and_then(Value::as_str) == Some("read"))
        .and_then(|action| action.get("path").and_then(Value::as_str))
        .filter(|path| is_well_formed_read_path(path))
        .map(|absolute| mapped_location(absolute, mapper))
        .map(|location| BoundedVec::new("commandLocations", vec![location]))
        .transpose()
        .map_err(content)?
        .unwrap_or_default();
    tool_with_summary_status(ToolPresentationInput {
        category: ToolCategory::Execute,
        label: &bounded_label(command.as_deref(), "Command"),
        tool_state: declared_tool_state(item)?,
        input_summary: command,
        output_summary: output,
        locations,
        command_activity: CommandActivity::Ordinary,
        execution: ToolExecution {
            cwd: command_cwd(item, mapper),
            exit_code: command_exit_code(item),
            ..Default::default()
        },
    })
}

fn is_well_formed_read_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= LIMITS.wire.absolute_path_bytes_native_only as usize
        && !path.contains('\0')
}

fn mapped_location<M: ResourceMapper>(
    absolute: &str,
    mapper: &M,
) -> crate::acp::intent::SafeResourceLocation {
    match mapper.map_existing(absolute) {
        Ok(resource) => crate::acp::intent::SafeResourceLocation::Resource { resource },
        Err(reason) => crate::acp::intent::SafeResourceLocation::unavailable(reason),
    }
}

fn image_generation(
    item: &Map<String, Value>,
    lifecycle: ItemLifecycle,
) -> Result<Option<ItemNormalization>, IngressError> {
    match item.get("result") {
        None | Some(Value::Null) if !lifecycle.is_terminal() => Ok(None),
        None | Some(Value::Null) => {
            rejected_image("imageGeneration", ImageAdmitError::MissingBytes)
        }
        Some(Value::String(url)) => match admit_image_data_url(url) {
            Ok(payload) => Ok(Some((payload, None, Some(ItemLifecycle::Completed)))),
            Err(reason) => rejected_image("imageGeneration", reason),
        },
        Some(_) => rejected_image("imageGeneration", ImageAdmitError::InvalidBase64),
    }
}

fn rejected_image(
    kind: &str,
    reason: ImageAdmitError,
) -> Result<Option<ItemNormalization>, IngressError> {
    Ok(Some((
        unsupported(kind, reason.unsupported_summary())?,
        None,
        Some(ItemLifecycle::Unsupported),
    )))
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
    let operation = match change
        .get("kind")
        .and_then(Value::as_object)
        .and_then(|kind| kind.get("type"))
        .and_then(Value::as_str)
    {
        Some("add") => crate::acp::intent::FileChangeOperation::Created,
        Some("delete") => crate::acp::intent::FileChangeOperation::Deleted,
        _ => crate::acp::intent::FileChangeOperation::Updated,
    };
    let resource = match mapper.map_existing(absolute) {
        Ok(resource) => crate::acp::intent::SafeResourceLocation::Resource { resource },
        Err(crate::acp::intent::UnavailableResourceReason::Missing)
            if operation == crate::acp::intent::FileChangeOperation::Deleted =>
        {
            match mapper.map_missing_display(absolute) {
                Some(display) => {
                    crate::acp::intent::SafeResourceLocation::unavailable_with_display(
                        crate::acp::intent::UnavailableResourceReason::Missing,
                        display,
                    )
                }
                None => crate::acp::intent::SafeResourceLocation::unavailable(
                    crate::acp::intent::UnavailableResourceReason::Missing,
                ),
            }
        }
        Err(reason) => crate::acp::intent::SafeResourceLocation::unavailable(reason),
    };
    let verb = match operation {
        crate::acp::intent::FileChangeOperation::Created => "Created",
        crate::acp::intent::FileChangeOperation::Updated => "Edited",
        crate::acp::intent::FileChangeOperation::Deleted => "Deleted",
    };
    let display = match &resource {
        crate::acp::intent::SafeResourceLocation::Resource { resource }
            if resource.relative_path.as_str().len() <= 480
                && !resource
                    .relative_path
                    .as_str()
                    .chars()
                    .any(char::is_control) =>
        {
            resource.relative_path.as_str()
        }
        _ => "file",
    };
    let summary = SafeText::new("fileSummary", format!("{verb} {display}")).map_err(content)?;
    // Codex `fileChange` wire evidence carries a single opaque `diff` string,
    // never a bounded old/new pair. Map to typed unavailable metadata without
    // inventing text from disk or Git; the card still opens its summary.
    Ok((
        NormalizedPayload::file_summary_only(
            crate::acp::intent::FileChangeFacts {
                resource,
                summary,
                additions: None,
                deletions: None,
                operation,
            },
            false,
        ),
        None,
        None,
    ))
}

#[cfg(test)]
mod tests;
