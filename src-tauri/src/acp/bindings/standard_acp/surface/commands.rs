use crate::acp::{
    bindings::adapter::AdapterRuntimeError,
    slash_commands::{
        AvailableCommand, CommandDispatch, CommandPanel, CommandSource, CommandState,
    },
};
use serde_json::Value;
use std::sync::Arc;

pub(crate) fn update_commands(
    value: &Value,
    session: &str,
    commands: &mut Arc<CommandState>,
) -> Result<bool, AdapterRuntimeError> {
    update_commands_with(value, session, commands, &[], |command| command)
}

/// A `first` row that is also one of the harness's own built-ins keeps the harness's
/// description and aliases; its route and input stay the binding's. A skill or plugin of that
/// name is not the same command, so the binding's row stays as it is.
pub(crate) fn update_commands_with(
    value: &Value,
    session: &str,
    commands: &mut Arc<CommandState>,
    first: &[AvailableCommand],
    decorate: impl Fn(Vec<AvailableCommand>) -> Vec<AvailableCommand>,
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
        let mut listed = listed;
        if first.is_empty() {
            return decorate(listed);
        }
        let routed: Vec<_> = first
            .iter()
            .map(|own| {
                let builtin = listed.iter().position(|harness| {
                    harness.name == own.name && harness.source == CommandSource::Builtin
                });
                match builtin {
                    Some(index) => AvailableCommand {
                        dispatch: own.dispatch.clone(),
                        input_hint: own.input_hint.clone(),
                        ..listed.remove(index)
                    },
                    None => own.clone(),
                }
            })
            .collect();
        decorate(routed.into_iter().chain(listed).collect())
    }))?;
    Ok(true)
}

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
