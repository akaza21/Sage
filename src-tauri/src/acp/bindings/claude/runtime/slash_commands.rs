use super::*;
use crate::acp::{
    bindings::standard_acp::surface::{panel, update_commands_with},
    command_placement::{NativeControl, place_commands},
    slash_commands::{AvailableCommand, CommandPanel, CommandSource},
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
        let active = self.active.as_ref().ok_or(AdapterRuntimeError::Frame)?;
        update_commands_with(
            value,
            active.external_id.as_str(),
            &active.commands,
            &first,
            |commands| {
                place_commands(
                    commands.into_iter().map(classify_command).collect(),
                    super::super::manifest::known_commands(),
                    self.command_version.as_ref(),
                    &|control| self.supports_command_control(control),
                )
            },
        )
    }

    /// Called on every list read; an unchanged projection allocates nothing.
    pub(super) fn project_terminal_handoff(&self) {
        if let Some(active) = &self.active {
            active.commands.update_terminal_handoff(
                super::super::manifest::terminal_metadata(),
                self.command_version.as_ref(),
                &|control| self.supports_command_control(control),
                Some(active.external_id.as_str()),
            );
        }
    }

    pub(super) fn recompute_command_placement(&self) -> Result<(), AdapterRuntimeError> {
        let active = self.active.as_ref().ok_or(AdapterRuntimeError::Frame)?;
        active.commands.recompute_placement(
            super::super::manifest::known_commands(),
            self.command_version.as_ref(),
            &|control| self.supports_command_control(control),
        )
    }

    fn supports_command_control(&self, control: NativeControl) -> bool {
        let Some(active) = self.active.as_ref() else {
            return false;
        };
        match control {
            NativeControl::Compact => {
                self.opens_panels && self.capabilities.as_ref().is_some_and(|c| c.prompt)
            }
            NativeControl::McpPanel | NativeControl::UsagePanel => self.opens_panels,
            NativeControl::NewChat => self.capabilities.as_ref().is_some_and(|c| c.new_session),
            NativeControl::ModelPicker => active
                .config
                .config_options
                .as_slice()
                .iter()
                .any(|o| o.category.as_ref().is_some_and(|c| c.as_str() == "model")),
            NativeControl::EffortPicker => {
                active.config.config_options.as_slice().iter().any(|o| {
                    o.category
                        .as_ref()
                        .is_some_and(|c| matches!(c.as_str(), "effort" | "thought_level"))
                })
            }
            NativeControl::AccessMenu => {
                active.config.modes.is_some()
                    || active
                        .config
                        .config_options
                        .as_slice()
                        .iter()
                        .any(|o| o.category.as_ref().is_some_and(|c| c.as_str() == "mode"))
            }
            _ => false,
        }
    }
}

/// Claude Code's own versions draw in its terminal, which a Sage session does not have. The
/// wording here is used only while Claude Code does not list the command itself.
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

/// Older ACP installs report plugin namespaces in command names only.
fn classify_command(mut command: AvailableCommand) -> AvailableCommand {
    if command.source == CommandSource::Harness
        && let Some((plugin, _)) = command.name.split_once(':')
    {
        let label = plugin.to_owned();
        command.source = CommandSource::Plugin;
        command = command.with_source_label(Some(&label));
    }
    command
}
