//! Sage never invents display names, titles, or results the item did not carry.

use serde_json::{Map, Value};

use crate::acp::{
    bindings::codex::state::command_explore::CommandExplore,
    contract::{BoundedVec, SafeText},
    declared_test::CommandActivity,
    intent::{SafeResourceLocation, ToolCategory, ToolExecution},
    lifecycle::ItemLifecycle,
};

use super::super::IngressError;
use super::presentation::{
    ToolLocations, ToolPresentationInput, bounded_label, bounded_summary, declared_tool_state,
    lenient_tool_state, tool_with_summary_status,
};
use super::{ItemNormalization, content};

/// Result text is clipped before sanitizing so the redacted form still fits
/// the 64 KiB tool-output bound; a smaller retry covers worst-case expansion.
const RESULT_CLIPS: [usize; 2] = [32_768, 8_192];
const TRUNCATED: &str = "\n\u{2026} truncated";

pub(super) fn mcp_tool(item: &Map<String, Value>) -> Result<ItemNormalization, IngressError> {
    let server = safe_name(item.get("server"));
    let tool = safe_name(item.get("tool"));
    let error = item
        .get("error")
        .and_then(Value::as_object)
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .filter(|message| !message.trim().is_empty());
    let output = match error {
        Some(message) => clipped_output(message),
        None => item
            .get("result")
            .and_then(mcp_result_text)
            .as_deref()
            .and_then(clipped_output),
    };
    tool_with_summary_status(ToolPresentationInput {
        category: ToolCategory::Mcp,
        label: tool.unwrap_or("MCP tool"),
        tool_state: declared_tool_state(item)?,
        input_summary: None,
        output_summary: output,
        locations: BoundedVec::default(),
        command_activity: CommandActivity::Ordinary,
        execution: ToolExecution {
            mcp_server: server
                .filter(|_| tool.is_some())
                .map(|server| SafeText::new("mcpServer", server))
                .transpose()
                .map_err(content)?
                .map(Box::new),
            ..Default::default()
        },
    })
}

pub(super) fn web_search(
    item: &Map<String, Value>,
    lifecycle: ItemLifecycle,
) -> Result<ItemNormalization, IngressError> {
    let action = item.get("action").and_then(Value::as_object);
    let query = [
        item.get("query"),
        action.and_then(|action| action.get("query")),
        action.and_then(|action| action.get("url")),
    ]
    .into_iter()
    .flatten()
    .filter_map(Value::as_str)
    .find(|query| !query.trim().is_empty() && !query.contains('\0'))
    .and_then(bounded_summary)
    .map(|query| bounded_label(Some(query.trim()), "Web search"));
    let results = item
        .get("results")
        .filter(|results| {
            results
                .as_array()
                .is_some_and(|results| !results.is_empty())
        })
        .and_then(|results| serde_json::to_string_pretty(results).ok())
        .as_deref()
        .and_then(clipped_output);
    tool_with_summary_status(ToolPresentationInput {
        category: ToolCategory::WebSearch,
        label: "Web search",
        tool_state: lenient_tool_state(item, lifecycle),
        input_summary: query,
        output_summary: results,
        locations: BoundedVec::default(),
        command_activity: CommandActivity::Ordinary,
        execution: ToolExecution::default(),
    })
}

pub(in crate::acp::bindings::codex::incoming) fn explore_presentation(
    explore: Option<&CommandExplore>,
    activity: CommandActivity,
    command: Option<&str>,
    locations: &ToolLocations,
) -> (ToolCategory, String) {
    let read_path = read_path(locations);
    match explore {
        _ if activity != CommandActivity::Ordinary => {
            (ToolCategory::Execute, bounded_label(command, "Command"))
        }
        Some(CommandExplore::Read(name)) => (
            ToolCategory::Read,
            match read_path.or(name.as_deref()) {
                Some(path) => bounded_label(Some(&format!("Read {path}")), "Read files"),
                None => bounded_label(command, "Read files"),
            },
        ),
        Some(CommandExplore::Search(label)) => (
            ToolCategory::Search,
            bounded_label(Some(label), "Search files"),
        ),
        None => (ToolCategory::Execute, bounded_label(command, "Command")),
    }
}

pub(in crate::acp::bindings::codex::incoming) fn read_path(
    locations: &ToolLocations,
) -> Option<&str> {
    match locations.as_slice() {
        [SafeResourceLocation::Resource { resource }] => Some(resource.relative_path.as_str()),
        _ => None,
    }
    .filter(|path| !path.chars().any(char::is_control))
}

fn safe_name(value: Option<&Value>) -> Option<&str> {
    let name = value?.as_str()?;
    (!name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.')))
    .then_some(name)
}

fn mcp_result_text(result: &Value) -> Option<String> {
    let result = result.as_object()?;
    if let Some(structured) = result
        .get("structuredContent")
        .filter(|value| !value.is_null())
    {
        return serde_json::to_string_pretty(structured).ok();
    }
    let blocks = result.get("content")?.as_array()?;
    let parts: Vec<String> = blocks
        .iter()
        .filter_map(|block| {
            let kind = block.get("type")?.as_str()?;
            Some(match block.get("text").and_then(Value::as_str) {
                Some(text) if kind == "text" => text.to_owned(),
                _ => format!("[{} content]", bounded_label(Some(kind), "unknown")),
            })
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join("\n\n"))
}

fn clipped_output(text: &str) -> Option<String> {
    RESULT_CLIPS.into_iter().find_map(|limit| {
        if text.len() <= limit {
            return bounded_summary(text);
        }
        let mut end = limit;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        bounded_summary(&format!("{}{TRUNCATED}", &text[..end]))
    })
}

#[cfg(test)]
mod tests {
    use super::clipped_output;

    #[test]
    fn oversized_results_are_clipped_with_a_visible_marker() {
        let clipped = clipped_output(&"a".repeat(40_000)).expect("clipped");
        assert!(clipped.len() < 33_000 && clipped.ends_with("\u{2026} truncated"));
        assert_eq!(clipped_output("small").as_deref(), Some("small"));
    }
}
