use super::*;
use crate::acp::{
    bindings::standard_acp::surface::{panel, update_commands_with},
    slash_commands::{AvailableCommand, CommandPanel},
};

impl ClaudeRuntime {
    pub(super) fn commands_notification(
        &mut self,
        value: &Value,
    ) -> Result<bool, AdapterRuntimeError> {
        if value["params"]["update"]["sessionUpdate"] != "available_commands_update" {
            return Ok(false);
        }
        let first = if self.opens_panels {
            panels()
        } else {
            Vec::new()
        };
        let active = self.active.as_mut().ok_or(AdapterRuntimeError::Frame)?;
        update_commands_with(
            value,
            active.external_id.as_str(),
            &mut active.commands,
            &first,
        )
    }
}

/// `/status` and `/mcp` as panels in the chat. Claude Code's own versions
/// draw in its terminal, which a Sage session does not have. The wording
/// here is used only while Claude Code does not list the command itself.
fn panels() -> Vec<AvailableCommand> {
    [
        panel(
            "status",
            "Show the session's model, access, context and usage",
            CommandPanel::Status,
        ),
        panel(
            "mcp",
            "Show MCP servers and reconnect them",
            CommandPanel::Mcp,
        ),
    ]
    .into_iter()
    .filter_map(Result::ok)
    .collect()
}
