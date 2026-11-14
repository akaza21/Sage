use serde_json::{Map, Value};

use super::StandardAcpTranslator;
use crate::acp::bindings::command_summary::sanitize_command_summary;
use sage_acp_protocol::{contract::SafeText, intent::ToolCategory};

/// ACP leaves `rawInput` free-form; an execute tool's `command` string is the
/// one shape agents share for the command line, so it is read for every
/// agent rather than per harness.
pub(super) fn command_input_patch(
    category: ToolCategory,
    update: &Map<String, Value>,
) -> Option<Option<SafeText<65536>>> {
    if category == ToolCategory::WebSearch {
        return web_search_query(update).map(sanitized_command_input);
    }
    if category != ToolCategory::Execute {
        return None;
    }
    let command = update.get("rawInput")?.as_object()?.get("command")?;
    Some(command.as_str().and_then(sanitized_command_input))
}

/// The exit status an execute tool reports in `rawOutput`, in the ACP
/// `TerminalExitStatus` spelling (`exitCode`) or the snake-case one agents
/// that run commands themselves use.
pub(super) fn command_exit_code(update: &Map<String, Value>) -> Option<i32> {
    let output = update.get("rawOutput")?.as_object()?;
    output
        .get("exitCode")
        .or_else(|| output.get("exit_code"))?
        .as_i64()
        .and_then(|code| i32::try_from(code).ok())
}

/// A standard ACP `fetch` tool whose raw input carries a query and no URL is
/// a web search. Only protocol fields decide this, never the harness name.
pub(super) fn web_search_query(update: &Map<String, Value>) -> Option<&str> {
    let input = update.get("rawInput")?.as_object()?;
    if input.contains_key("url") {
        return None;
    }
    input
        .get("query")?
        .as_str()
        .filter(|query| !query.trim().is_empty())
}

impl StandardAcpTranslator {
    pub(super) fn set_command_input(&mut self, id: &str, input: Option<SafeText<65536>>) {
        match input {
            Some(input) => self.tool_inputs.insert(id.to_owned(), input),
            None => self.tool_inputs.remove(id),
        };
    }
}

fn sanitized_command_input(command: &str) -> Option<SafeText<65536>> {
    let shown = sanitize_command_summary(command).ok()?;
    let text = shown.as_str().trim();
    if text.is_empty() {
        return None;
    }
    SafeText::new("toolInput", text).ok()
}
