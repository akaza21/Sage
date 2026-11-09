use crate::acp::{bindings::adapter::AdapterRuntimeError, slash_commands::CommandState};
use serde_json::Value;
use std::sync::Arc;
pub(crate) fn update_commands(
    value: &Value,
    session: &str,
    commands: &mut Arc<CommandState>,
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
    commands.replace(super::command_list::parse_acp(
        &value["params"]["update"]["availableCommands"],
    ))?;
    Ok(true)
}
