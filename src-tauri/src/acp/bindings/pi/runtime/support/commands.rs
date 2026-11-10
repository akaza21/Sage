use serde_json::Value;

use crate::acp::{
    bindings::{
        adapter::{AdapterEgress, AdapterEvent, AdapterRuntimeError},
        standard_acp::surface::command_list::{
            AvailableCommand, CommandDispatch, CommandSource, MAX_COMMANDS, wire_text,
        },
    },
    intent::Prompt,
};

use super::super::{PiRuntime, RequestKind};
use crate::acp::bindings::pi::Operation;

/// Pi's own commands that RPC runs by name rather than as a prompt. As in
/// Pi's CLI, they take the place of a template or skill of the same name.
const COMPACT: &str = "compact";

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

/// Pi's commands with its built-ins added, so they list like any other. A
/// catalog larger than Sage's list keeps its first commands, giving the last
/// place to compact.
fn with_builtins(value: &Value) -> Result<Vec<AvailableCommand>, AdapterRuntimeError> {
    let mut commands = value
        .as_array()
        .ok_or(AdapterRuntimeError::Frame)?
        .iter()
        .filter(|value| value["name"] != COMPACT)
        .take(MAX_COMMANDS - 1)
        .map(command)
        .collect::<Result<Vec<_>, _>>()?;
    commands.push(AvailableCommand::new(
        COMPACT,
        "Summarize the conversation so far to free up context",
        Some("what the summary should focus on"),
        CommandSource::Builtin,
        CommandDispatch::Builtin,
    )?);
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

    /// Sends a prompt as Pi expects it: a built-in command by its own request,
    /// an extension command as a command turn, anything else as a prompt.
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
            Some((listed.dispatch, listed.name.as_str(), rest))
        });
        let turn_key = turn_key.to_owned();
        let operation = match command {
            Some((CommandDispatch::Builtin, COMPACT, instructions)) => Operation::Compact {
                turn_key,
                instructions: (!instructions.is_empty()).then(|| instructions.to_owned()),
            },
            _ => Operation::Prompt {
                turn_key,
                message: prompt.as_str().to_owned(),
            },
        };
        let egress = self.begin_one(operation, RequestKind::Prompt)?;
        if matches!(command, Some((CommandDispatch::CommandTurn, ..))) {
            self.translator.mark_command_turn().map_err(super::map_pi)?;
        }
        Ok(egress)
    }
}
