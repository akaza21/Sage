//! Ephemeral, bounded command advertisements. No durable history or inferred commands.
use crate::acp::{
    bindings::adapter::AdapterRuntimeError,
    contract::{RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId},
    scheduler::RuntimeOwner,
};
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
    fn revision(&self) -> Option<u64> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .map(|list| list.revision)
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

/// Tells the WebView that one live owner's command list has a new revision,
/// so it re-reads the bounded list. It carries no command data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CommandsChangedV1 {
    pub(crate) schema_version: u8,
    pub(crate) sage_project_id: SageProjectId,
    pub(crate) sage_chat_id: SageChatId,
    pub(crate) sage_agent_session_id: SageAgentSessionId,
    pub(crate) runtime_epoch: RuntimeEpoch,
    pub(crate) revision: u64,
}

pub(crate) type CommandNoticeSink = Arc<dyn Fn(CommandsChangedV1) + Send + Sync>;

struct Slot {
    owner: RuntimeOwner,
    state: Arc<CommandState>,
    announced: u64,
}

/// One slot per bounded live runtime lane. Old epochs cannot publish or clear a new owner.
#[derive(Default)]
pub(crate) struct CommandChannel {
    slot: Mutex<Option<Slot>>,
    sink: Mutex<Option<CommandNoticeSink>>,
}
impl CommandChannel {
    fn slot(&self) -> std::sync::MutexGuard<'_, Option<Slot>> {
        self.slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    pub(crate) fn set_sink(&self, sink: CommandNoticeSink) {
        *self
            .sink
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(sink);
    }
    pub(crate) fn reserve(&self, owner: RuntimeOwner) {
        let mut slot = self.slot();
        if let Some(previous) = slot.take() {
            previous.state.close();
        }
        *slot = Some(Slot {
            owner,
            state: Arc::default(),
            announced: 0,
        });
    }
    /// Adopts the owner's live list and announces each new revision once.
    pub(crate) fn update(&self, owner: &RuntimeOwner, list: Arc<CommandState>) {
        let revision = {
            let mut slot = self.slot();
            let Some(current) = slot.as_mut().filter(|s| &s.owner == owner) else {
                return;
            };
            if !Arc::ptr_eq(&current.state, &list) {
                // Another list counts its revisions afresh (OpenCode can hand
                // over a new empty state), so its first revision is news.
                current.state.close();
                current.state = list;
                current.announced = 0;
            }
            match current.state.revision() {
                // Revision 0 is the empty list before any advertisement.
                Some(revision) if revision != 0 && revision != current.announced => {
                    current.announced = revision;
                    revision
                }
                _ => return,
            }
        };
        let sink = self
            .sink
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if let Some(sink) = sink {
            sink(CommandsChangedV1 {
                schema_version: 1,
                sage_project_id: owner.project_id.clone(),
                sage_chat_id: owner.chat_id.clone(),
                sage_agent_session_id: owner.session_id.clone(),
                runtime_epoch: owner.runtime_epoch,
                revision,
            });
        }
    }
    pub(crate) fn snapshot(&self, owner: &RuntimeOwner) -> Option<Arc<CommandState>> {
        self.slot()
            .as_ref()
            .filter(|s| &s.owner == owner)
            .map(|s| Arc::clone(&s.state))
    }
    pub(crate) fn clear(&self, owner: &RuntimeOwner) {
        let mut slot = self.slot();
        if slot.as_ref().is_some_and(|s| &s.owner == owner)
            && let Some(previous) = slot.take()
        {
            previous.state.close();
        }
    }
    pub(crate) fn clear_all(&self) {
        if let Some(previous) = self.slot().take() {
            previous.state.close();
        }
    }
}

#[cfg(test)]
mod tests;
