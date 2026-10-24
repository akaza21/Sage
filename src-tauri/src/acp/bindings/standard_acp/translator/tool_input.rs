use serde_json::{Map, Value};

use super::super::profile::StandardAcpProfile;
use super::StandardAcpTranslator;
use crate::acp::{
    bindings::command_summary::sanitize_command_summary, contract::SafeText, intent::ToolCategory,
};

pub(super) fn command_input_patch(
    profile: &StandardAcpProfile,
    category: ToolCategory,
    update: &Map<String, Value>,
) -> Option<Option<SafeText<65536>>> {
    if category == ToolCategory::WebSearch {
        return web_search_query(update).map(sanitized_command_input);
    }
    if category != ToolCategory::Execute {
        return None;
    }
    let field = profile.command_input_field?;
    let command = update.get("rawInput")?.as_object()?.get(field)?;
    Some(command.as_str().and_then(sanitized_command_input))
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
