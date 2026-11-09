use serde_json::{Value, json};

use crate::acp::{
    bindings::{
        adapter::{AdapterEgress, AdapterEvent, AdapterRuntimeError},
        standard_acp::surface::command_list::MAX_COMMANDS,
    },
    intent::Prompt,
};

use super::super::{PiRuntime, RequestKind};
use crate::acp::bindings::pi::Operation;

/// Pi's own commands that RPC runs by name rather than as a prompt. As in
/// Pi's CLI, they take the place of a template or skill of the same name.
const COMPACT: &str = "compact";

/// Pi's commands with its built-ins added, so they list like any other.
fn with_builtins(value: Value) -> Value {
    let Value::Array(mut commands) = value else {
        return value;
    };
    commands.retain(|command| command["name"] != COMPACT);
    // A full catalog keeps its size, giving its last entry's place to compact.
    if commands.len() == MAX_COMMANDS {
        commands.pop();
    }
    commands.push(json!({
        "name": COMPACT,
        "description": "Summarize the conversation so far to free up context",
        "input": {"hint": "what the summary should focus on"},
    }));
    Value::Array(commands)
}

impl PiRuntime {
    pub(super) fn list_commands(
        &mut self,
        value: Value,
    ) -> Result<AdapterEvent, AdapterRuntimeError> {
        self.commands_pending = false;
        self.commands.replace(&with_builtins(value), true)?;
        Ok(AdapterEvent::Absorbed)
    }

    /// Sends a prompt as Pi expects it: a built-in command by its own request,
    /// an extension command as a command turn, anything else as a prompt.
    pub(in crate::acp::bindings::pi::runtime) fn begin_prompt(
        &mut self,
        prompt: &Prompt,
        turn_key: &str,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        let command = prompt
            .command_revision
            .and_then(|_| prompt.as_str().strip_prefix('/'))
            .and_then(|text| {
                let name = text.split_whitespace().next()?;
                let listed = self.commands.snapshot().commands;
                let listed = listed.iter().find(|command| command.name == name)?;
                let rest = text.trim_start().strip_prefix(name)?.trim();
                Some((name == COMPACT, listed.extension, rest))
            });
        let turn_key = turn_key.to_owned();
        let operation = match command {
            Some((true, _, instructions)) => Operation::Compact {
                turn_key,
                instructions: (!instructions.is_empty()).then(|| instructions.to_owned()),
            },
            _ => Operation::Prompt {
                turn_key,
                message: prompt.resources.inline(prompt.as_str()),
            },
        };
        let egress = self.begin_one(operation, RequestKind::Prompt)?;
        if matches!(command, Some((false, true, _))) {
            self.translator.mark_command_turn().map_err(super::map_pi)?;
        }
        Ok(egress)
    }
}
