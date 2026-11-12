//! A harness's ACP command list, and the binding's own commands that join
//! it: rows that open a panel in the chat instead of sending a prompt.

use crate::acp::{
    bindings::adapter::AdapterRuntimeError,
    slash_commands::{
        AvailableCommand, CommandDispatch, CommandPanel, CommandSource, CommandState,
    },
};
use serde_json::Value;
use std::sync::Arc;

use super::command_list::{MAX_COMMAND_BYTES, MAX_COMMANDS};

pub(crate) fn update_commands(
    value: &Value,
    session: &str,
    commands: &mut Arc<CommandState>,
) -> Result<bool, AdapterRuntimeError> {
    update_commands_with(value, session, commands, &[])
}

/// An `available_commands_update` for `session`, with the binding's own
/// `first` rows ahead of the harness's. A `first` row the harness lists too
/// keeps the harness's wording and takes only its route from the binding,
/// so the menu shows the command as the harness describes it.
pub(crate) fn update_commands_with(
    value: &Value,
    session: &str,
    commands: &mut Arc<CommandState>,
    first: &[AvailableCommand],
) -> Result<bool, AdapterRuntimeError> {
    if value["params"]["update"]["sessionUpdate"] != "available_commands_update" {
        return Ok(false);
    }
    if value["jsonrpc"] != "2.0"
        || value["method"] != "session/update"
        || value.get("id").is_some()
        || value.get("result").is_some()
        || value.get("error").is_some()
        || value["params"]["sessionId"].as_str() != Some(session)
    {
        return Err(AdapterRuntimeError::Frame);
    }
    let parsed = super::command_list::parse_acp(&value["params"]["update"]["availableCommands"]);
    commands.replace(parsed.map(|listed| {
        if first.is_empty() {
            return listed;
        }
        let routed: Vec<_> = first
            .iter()
            .map(
                |own| match listed.iter().find(|harness| harness.name == own.name) {
                    Some(harness) => AvailableCommand {
                        dispatch: own.dispatch.clone(),
                        ..harness.clone()
                    },
                    None => own.clone(),
                },
            )
            .collect();
        first_listed(routed.into_iter().chain(listed))
    }))?;
    Ok(true)
}

/// A built-in row that opens `panel` and sends nothing.
pub(crate) fn panel(
    name: &str,
    description: &str,
    panel: CommandPanel,
) -> Result<AvailableCommand, AdapterRuntimeError> {
    AvailableCommand::new(
        name,
        description,
        None,
        CommandSource::Builtin,
        CommandDispatch::Panel(panel),
    )
}

/// The first command of each name, ending at the first command past the
/// shared count or byte bound.
pub(crate) fn first_listed(
    commands: impl IntoIterator<Item = AvailableCommand>,
) -> Vec<AvailableCommand> {
    let mut names = std::collections::HashSet::new();
    let mut bytes = 0usize;
    commands
        .into_iter()
        .filter(|command| names.insert(command.name.clone()))
        .take(MAX_COMMANDS)
        .take_while(|command| {
            bytes += command.listed_bytes();
            bytes <= MAX_COMMAND_BYTES
        })
        .collect()
}
