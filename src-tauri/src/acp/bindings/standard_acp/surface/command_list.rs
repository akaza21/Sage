use crate::acp::{
    bindings::adapter::AdapterRuntimeError,
    contract::{RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId},
    scheduler::RuntimeOwner,
};
use serde::Serialize;
use std::sync::{Arc, Mutex};

/// Room for a harness with many skills, plugins and MCP prompts; the byte
/// cap bounds the list's memory whatever the count.
pub(crate) const MAX_COMMANDS: usize = 512;
pub(crate) const MAX_COMMAND_BYTES: usize = 256 * 1024;

mod entry;
pub(crate) use entry::*;

fn admit(commands: Vec<AvailableCommand>) -> Result<Vec<AvailableCommand>, AdapterRuntimeError> {
    if commands.len() > MAX_COMMANDS {
        return Err(AdapterRuntimeError::BatchCount);
    }
    let mut bytes = 0;
    let mut names = std::collections::HashSet::with_capacity(commands.len());
    for command in &commands {
        if !names.insert(command.name.as_str()) {
            return Err(AdapterRuntimeError::Frame);
        }
        bytes += command.listed_bytes();
        if bytes > MAX_COMMAND_BYTES {
            return Err(AdapterRuntimeError::BatchBytes);
        }
    }
    Ok(commands)
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct CommandList {
    pub(crate) revision: u64,
    pub(crate) commands: Vec<AvailableCommand>,
}

impl CommandList {
    /// A failed parse still counts as a new revision and leaves the list empty, so stale command
    /// authority is never usable.
    pub(crate) fn replace(
        &mut self,
        parsed: Result<Vec<AvailableCommand>, AdapterRuntimeError>,
    ) -> Result<(), AdapterRuntimeError> {
        self.commands = Vec::new();
        self.revision = self
            .revision
            .checked_add(1)
            .filter(|revision| *revision <= 9_007_199_254_740_991)
            .ok_or(AdapterRuntimeError::Operation)?;
        self.commands = admit(parsed?)?;
        Ok(())
    }
    pub(crate) fn find(&self, revision: u64, text: &str) -> Option<&AvailableCommand> {
        let name = text.strip_prefix('/')?.split_whitespace().next()?;
        (revision == self.revision)
            .then(|| self.commands.iter().find(|c| c.name == name))
            .flatten()
    }
    pub(crate) fn validate(&self, revision: u64, text: &str) -> Result<(), AdapterRuntimeError> {
        self.find(revision, text)
            .map(|_| ())
            .ok_or(AdapterRuntimeError::UnsupportedCommand)
    }
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
    pub(crate) fn replace(
        &self,
        parsed: Result<Vec<AvailableCommand>, AdapterRuntimeError>,
    ) -> Result<(), AdapterRuntimeError> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_mut()
            .ok_or(AdapterRuntimeError::Operation)?
            .replace(parsed)
    }
    /// Takes a binding's new list unless it equals the shown one, so an
    /// unchanged list keeps its revision and a pick made from it still runs.
    pub(crate) fn replace_if_changed(
        &self,
        commands: Vec<AvailableCommand>,
    ) -> Result<(), AdapterRuntimeError> {
        let mut list = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let list = list.as_mut().ok_or(AdapterRuntimeError::Operation)?;
        if list.commands == commands {
            return Ok(());
        }
        list.replace(Ok(commands))
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
    pub(crate) fn command(
        &self,
        revision: u64,
        text: &str,
    ) -> Result<AvailableCommand, AdapterRuntimeError> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .ok_or(AdapterRuntimeError::Operation)?
            .find(revision, text)
            .cloned()
            .ok_or(AdapterRuntimeError::UnsupportedCommand)
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
