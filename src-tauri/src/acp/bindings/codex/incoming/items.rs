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

mod file_change;
mod presentation;
mod runtime_rows;

pub(super) use file_change::{
    edit_tool as file_change_edit_tool, normalize as normalize_file_change,
};
mod tool_kinds;

use tool_kinds::{explore_presentation, mcp_tool, web_search};

use presentation::{
    ToolPresentationInput, bounded_label, bounded_summary, context_compaction, declared_tool_state,
    lenient_tool_state, parse_tool_state, tool, tool_with_summary, tool_with_summary_status,
};

pub(super) fn normalize_materialized_command(
    command: &MaterializedCommandItem,
) -> Result<ItemNormalization, IngressError> {
    let (category, label) = explore_presentation(
        command.explore.as_ref(),
        command.activity,
        command.command.as_deref(),
        &command.locations,
    );
    let (payload, tool_state, terminal) = tool_with_summary_status(ToolPresentationInput {
        category,
        label: &label,
        tool_state: parse_tool_state(command.status.as_str()).ok_or(IngressError::Malformed)?,
        input_summary: command
            .command
            .clone()
            .filter(|_| category != ToolCategory::Read),
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
        "mcpToolCall" => mcp_tool(item).map(Some),
        "contextCompaction" => context_compaction(item, lifecycle).map(Some),
        "dynamicToolCall" => dynamic_tool(item).map(Some),
        "webSearch" => web_search(item, lifecycle).map(Some),
        "collabAgentToolCall" | "subAgentActivity" => presentation::collaboration(item).map(Some),
        "fileChange" if !lifecycle.is_terminal() => Ok(None),
        "fileChange" => normalize_file_change(item, mapper, false).map(|(item, _)| Some(item)),
        "imageGeneration" => image_generation(item, lifecycle),
        "imageView" => image_view(item, mapper, lifecycle).map(Some),
        "enteredReviewMode" => runtime_rows::entered_review(item, lifecycle).map(Some),
        "exitedReviewMode" => runtime_rows::exited_review(item),
        "sleep" => runtime_rows::sleep(item, lifecycle).map(Some),
        "functionCallOutput" => runtime_rows::function_call_output(item, lifecycle).map(Some),
        "hookPrompt" => runtime_rows::hook_prompt(item, lifecycle).map(Some),
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
        category: ToolCategory::ImageView,
        label: "Viewed an image",
        tool_state: lenient_tool_state(item, lifecycle),
        input_summary: None,
        output_summary: None,
        locations,
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

/// Codex shows the reasoning summary only; raw reasoning `content` is never
/// projected into the transcript.
fn reasoning(item: &Map<String, Value>) -> Result<Option<ItemNormalization>, IngressError> {
    let parts = item
        .get("summary")
        .and_then(Value::as_array)
        .into_iter()
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
    let explore = item
        .get("commandActions")
        .and_then(crate::acp::bindings::codex::state::command_explore::classify);
    let (category, label) = explore_presentation(
        explore.as_ref(),
        CommandActivity::Ordinary,
        command.as_deref(),
        &locations,
    );
    tool_with_summary_status(ToolPresentationInput {
        category,
        label: &label,
        tool_state: declared_tool_state(item)?,
        input_summary: command.filter(|_| category != ToolCategory::Read),
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

#[cfg(test)]
mod tests;
