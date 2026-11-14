use serde_json::Value;

use crate::acp::{
    bindings::{
        adapter::{AdapterEgress, AdapterEvent, AdapterRuntimeError},
        standard_acp::surface::command_list::{
            AvailableCommand, CommandDispatch, CommandPanel, CommandSource, MAX_COMMANDS, wire_text,
        },
    },
    intent::Prompt,
    session_action::SessionAction,
};

use super::super::{PiRuntime, RequestKind};
use crate::acp::bindings::pi::Operation;

/// Pi's built-in commands take precedence over templates or skills of the same name. RPC does
/// not list them, so their wording comes from Pi's command table.
const COMPACT: &str = "compact";
const SESSION: &str = "session";

/// One entry of Pi's `get_commands`. Its `source` says what the command
/// is; only extension commands run as command turns.
fn command(value: &Value) -> Result<AvailableCommand, AdapterRuntimeError> {
    let source = CommandSource::from_wire(value["source"].as_str());
    let dispatch = match source {
        CommandSource::Extension => CommandDispatch::CommandTurn,
        _ => CommandDispatch::Prompt,
    };
    let description = match value.get("description") {
        None => "",
        Some(description) => wire_text(description)?,
    };
    AvailableCommand::new(
        wire_text(&value["name"])?,
        description,
        None,
        source,
        dispatch,
    )
}

/// A catalog larger than Sage's list keeps its first commands, leaving room for the built-ins.
fn with_builtins(value: &Value) -> Result<Vec<AvailableCommand>, AdapterRuntimeError> {
    let builtins = [
        AvailableCommand::new(
            SESSION,
            "Show session info and stats",
            None,
            CommandSource::Builtin,
            CommandDispatch::Panel(CommandPanel::Status),
        )?,
        AvailableCommand::new(
            COMPACT,
            "Manually compact the session context",
            None,
            CommandSource::Builtin,
            CommandDispatch::Builtin,
        )?,
    ];
    let mut commands = value
        .as_array()
        .ok_or(AdapterRuntimeError::Frame)?
        .iter()
        .filter(|value| {
            builtins
                .iter()
                .all(|builtin| value["name"] != builtin.name.as_str())
        })
        .take(MAX_COMMANDS - builtins.len())
        .map(command)
        .collect::<Result<Vec<_>, _>>()?;
    commands.extend(builtins);
    Ok(commands)
}

impl PiRuntime {
    pub(super) fn list_commands(
        &mut self,
        value: Value,
    ) -> Result<AdapterEvent, AdapterRuntimeError> {
        self.commands_pending = false;
        self.commands.replace(with_builtins(&value))?;
        Ok(AdapterEvent::Absorbed)
    }

    pub(in crate::acp::bindings::pi::runtime) fn begin_prompt(
        &mut self,
        prompt: &Prompt,
        turn_key: &str,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        let listed = self.commands.snapshot();
        let command = prompt.command_revision.and_then(|revision| {
            let text = prompt.as_str();
            let listed = listed.find(revision, text)?;
            let rest = text[1..].trim_start().strip_prefix(&listed.name)?.trim();
            Some((listed.dispatch.clone(), listed.name.as_str(), rest))
        });
        let command_turn =
            prompt.action.is_none() && matches!(command, Some((CommandDispatch::CommandTurn, ..)));
        let turn_key = turn_key.to_owned();
        let operation = match (&prompt.action, command) {
            (Some(SessionAction::Compact {}), _) => Operation::Compact {
                turn_key,
                instructions: None,
            },
            (Some(SessionAction::Review { .. }), _) => {
                return Err(AdapterRuntimeError::UnsupportedCommand);
            }
            (None, Some((CommandDispatch::Builtin, COMPACT, instructions))) => Operation::Compact {
                turn_key,
                instructions: (!instructions.is_empty()).then(|| instructions.to_owned()),
            },
            (None, _) => Operation::Prompt {
                turn_key,
                message: prompt.resources.inline(prompt.as_str()),
                // Pi repeats the images in its own events, so they take the
                // echoed bound.
                images: prompt
                    .images
                    .echoed()
                    .map_err(|_| AdapterRuntimeError::UnsupportedCommand)?
                    .into_iter()
                    .map(|(media_type, data)| {
                        serde_json::json!({"type": "image", "data": data, "mimeType": media_type})
                    })
                    .collect(),
            },
        };
        let egress = self.begin_one(operation, RequestKind::Prompt)?;
        if command_turn {
            self.translator.mark_command_turn().map_err(super::map_pi)?;
        }
        Ok(egress)
    }
}
