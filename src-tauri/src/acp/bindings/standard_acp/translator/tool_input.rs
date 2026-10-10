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
    if category != ToolCategory::Execute {
        return None;
    }
    let field = profile.command_input_field?;
    let command = update.get("rawInput")?.as_object()?.get(field)?;
    Some(command.as_str().and_then(bounded_command_summary))
}

impl StandardAcpTranslator {
    pub(super) fn set_command_input(&mut self, id: &str, input: Option<SafeText<65536>>) {
        match input {
            Some(input) => self.tool_inputs.insert(id.to_owned(), input),
            None => self.tool_inputs.remove(id),
        };
    }
}

fn bounded_command_summary(command: &str) -> Option<SafeText<65536>> {
    let shown = sanitize_command_summary(command).ok()?;
    let text = shown.as_str().trim();
    if text.is_empty() {
        return None;
    }
    if text.len() <= 512 {
        return SafeText::new("toolInput", text).ok();
    }
    let mut end = 509;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    SafeText::new("toolInput", format!("{}\u{2026}", text[..end].trim_end())).ok()
}
