//! Ephemeral, bounded command advertisements. No durable history or inferred commands.
use crate::acp::{bindings::adapter::AdapterRuntimeError, scheduler::RuntimeOwner};
use serde::Serialize;
use serde_json::Value;
use std::sync::{Arc, Mutex};

pub(crate) const MAX_COMMANDS: usize = 128;
pub(crate) const MAX_COMMAND_BYTES: usize = 128 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AvailableCommand {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) input_hint: Option<String>,
    #[serde(skip)]
    pub(crate) extension: bool,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct CommandList {
    pub(crate) revision: u64,
    pub(crate) commands: Vec<AvailableCommand>,
}

impl CommandList {
    pub(crate) fn replace(&mut self, value: &Value, pi: bool) -> Result<(), AdapterRuntimeError> {
        // An invalid replacement must never leave stale command authority usable.
        self.commands = Vec::new();
        self.revision = self
            .revision
            .checked_add(1)
            .filter(|revision| *revision <= 9_007_199_254_740_991)
            .ok_or(AdapterRuntimeError::Operation)?;
        self.commands = parse(value, pi)?;
        Ok(())
    }
    pub(crate) fn validate(&self, revision: u64, text: &str) -> Result<(), AdapterRuntimeError> {
        let name = text
            .strip_prefix('/')
            .and_then(|s| s.split_whitespace().next())
            .ok_or(AdapterRuntimeError::Operation)?;
        if revision != self.revision || !self.commands.iter().any(|c| c.name == name) {
            return Err(AdapterRuntimeError::UnsupportedCommand);
        }
        Ok(())
    }
}

fn text(value: &Value, cap: usize) -> Result<String, AdapterRuntimeError> {
    let s = value.as_str().ok_or(AdapterRuntimeError::Frame)?;
    if s.len() > cap || s.contains('\0') {
        return Err(AdapterRuntimeError::BatchBytes);
    }
    Ok(s.to_owned())
}
fn parse(value: &Value, pi: bool) -> Result<Vec<AvailableCommand>, AdapterRuntimeError> {
    let values = value
        .as_array()
        .filter(|v| v.len() <= MAX_COMMANDS)
        .ok_or(AdapterRuntimeError::BatchCount)?;
    let mut commands: Vec<AvailableCommand> = Vec::with_capacity(values.len());
    let mut bytes = 0;
    for value in values {
        let name = text(&value["name"], 128)?;
        if name.is_empty()
            || name
                .chars()
                .any(|c| c.is_whitespace() || c.is_control() || c == '/')
            || commands.iter().any(|c| c.name == name)
        {
            return Err(AdapterRuntimeError::Frame);
        }
        let description = if pi && value.get("description").is_none() {
            String::new()
        } else {
            text(&value["description"], 1024)?
        };
        let input_hint = match value.get("input").filter(|v| !v.is_null()) {
            Some(input) => Some(text(&input["hint"], 256)?),
            None => None,
        };
        bytes += name.len() + description.len() + input_hint.as_ref().map_or(0, String::len);
        if bytes > MAX_COMMAND_BYTES {
            return Err(AdapterRuntimeError::BatchBytes);
        }
        commands.push(AvailableCommand {
            name,
            description,
            input_hint,
            extension: pi && value["source"] == "extension",
        });
    }
    Ok(commands)
}

/// Shared with the runtime lane so process/project teardown revokes and frees
/// command data even when a failed driver remains retained for recovery.
pub(crate) struct CommandState(Mutex<Option<CommandList>>);
impl Default for CommandState {
    fn default() -> Self {
        Self(Mutex::new(Some(CommandList::default())))
    }
}
impl CommandState {
    pub(crate) fn unsupported() -> Arc<Self> {
        static EMPTY: std::sync::OnceLock<Arc<CommandState>> = std::sync::OnceLock::new();
        Arc::clone(EMPTY.get_or_init(|| Arc::new(CommandState(Mutex::new(None)))))
    }
    pub(crate) fn replace(&self, value: &Value, pi: bool) -> Result<(), AdapterRuntimeError> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_mut()
            .ok_or(AdapterRuntimeError::Operation)?
            .replace(value, pi)
    }
    pub(crate) fn snapshot(&self) -> CommandList {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .unwrap_or_default()
    }
    pub(crate) fn validate(&self, revision: u64, text: &str) -> Result<(), AdapterRuntimeError> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .ok_or(AdapterRuntimeError::Operation)?
            .validate(revision, text)
    }
    fn close(&self) {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
    }
}

/// One slot per bounded live runtime lane. Old epochs cannot publish or clear a new owner.
#[derive(Default)]
pub(crate) struct CommandChannel(Mutex<Option<(RuntimeOwner, Arc<CommandState>)>>);
impl CommandChannel {
    pub(crate) fn reserve(&self, owner: RuntimeOwner) {
        let mut slot = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((_, previous)) = slot.take() {
            previous.close();
        }
        *slot = Some((owner, Arc::default()));
    }
    pub(crate) fn update(&self, owner: &RuntimeOwner, list: Arc<CommandState>) {
        if let Some((current, target)) = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_mut()
            && current == owner
            && !Arc::ptr_eq(target, &list)
        {
            target.close();
            *target = list;
        }
    }
    pub(crate) fn snapshot(&self, owner: &RuntimeOwner) -> Option<Arc<CommandState>> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .filter(|(current, _)| current == owner)
            .map(|(_, list)| Arc::clone(list))
    }
    pub(crate) fn clear(&self, owner: &RuntimeOwner) {
        let mut slot = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if slot.as_ref().is_some_and(|(current, _)| current == owner)
            && let Some((_, state)) = slot.take()
        {
            state.close();
        }
    }
    pub(crate) fn clear_all(&self) {
        if let Some((_, state)) = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            state.close();
        }
    }
}

#[cfg(test)]
mod tests;
