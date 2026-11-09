//! Ephemeral, bounded command advertisements. No durable history or inferred commands.
use crate::acp::{
    bindings::adapter::AdapterRuntimeError,
    contract::{RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId},
    scheduler::RuntimeOwner,
};
use serde::Serialize;
use serde_json::Value;
use std::sync::{Arc, Mutex};

/// Room for a harness with many skills, plugins and MCP prompts; the byte
/// cap bounds the list's memory whatever the count.
pub(crate) const MAX_COMMANDS: usize = 512;
pub(crate) const MAX_COMMAND_BYTES: usize = 256 * 1024;
const NAME_BYTES: usize = 128;
const DESCRIPTION_BYTES: usize = 1024;
const HINT_BYTES: usize = 256;

/// Where a command comes from, as its harness reports it. The `/` menu
/// groups and labels commands by it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CommandSource {
    /// The harness's own command.
    Builtin,
    Skill,
    /// A user or project prompt template.
    Prompt,
    Extension,
    /// A prompt an MCP server offers.
    Mcp,
    Plugin,
    /// A command the harness did not classify.
    Harness,
}

/// How the binding that advertised a command runs it. Native only: the
/// WebView names a command and the binding looks its dispatch up in the
/// live list, so a prompt can never choose its own route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandDispatch {
    /// Sent as `/name input` prompt text, which the harness parses.
    Prompt,
    /// Sent as prompt text that the harness runs as a command turn, which
    /// may finish without model work (Pi's extension commands).
    CommandTurn,
    /// Run by the binding's own method for this name (Pi's `compact`).
    Builtin,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AvailableCommand {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) input_hint: Option<String>,
    pub(crate) source: CommandSource,
    #[serde(skip)]
    pub(crate) dispatch: CommandDispatch,
}

impl AvailableCommand {
    /// One advertised command, with the bounds every list shares.
    pub(crate) fn new(
        name: &str,
        description: &str,
        input_hint: Option<&str>,
        source: CommandSource,
        dispatch: CommandDispatch,
    ) -> Result<Self, AdapterRuntimeError> {
        if name.is_empty()
            || name
                .chars()
                .any(|c| c.is_whitespace() || c.is_control() || c == '/')
        {
            return Err(AdapterRuntimeError::Frame);
        }
        Ok(Self {
            name: bounded(name, NAME_BYTES)?,
            description: clipped(description, DESCRIPTION_BYTES)?,
            input_hint: input_hint
                .map(|hint| bounded(hint, HINT_BYTES))
                .transpose()?,
            source,
            dispatch,
        })
    }

    /// An ACP `AvailableCommand`. ACP has no source field, so an adapter
    /// may name one in `_meta.source`; anything else is the harness's own.
    pub(crate) fn from_acp(value: &Value) -> Result<Self, AdapterRuntimeError> {
        let source = match value["_meta"]["source"].as_str() {
            Some("builtin") => CommandSource::Builtin,
            Some("skill") => CommandSource::Skill,
            Some("prompt") => CommandSource::Prompt,
            Some("extension") => CommandSource::Extension,
            Some("mcp") => CommandSource::Mcp,
            Some("plugin") => CommandSource::Plugin,
            _ => CommandSource::Harness,
        };
        let input_hint = match value.get("input").filter(|v| !v.is_null()) {
            Some(input) => Some(wire_text(&input["hint"])?),
            None => None,
        };
        Self::new(
            wire_text(&value["name"])?,
            wire_text(&value["description"])?,
            input_hint,
            source,
            CommandDispatch::Prompt,
        )
    }
}

/// A required string field of an advertisement.
pub(crate) fn wire_text(value: &Value) -> Result<&str, AdapterRuntimeError> {
    value.as_str().ok_or(AdapterRuntimeError::Frame)
}

fn bounded(text: &str, cap: usize) -> Result<String, AdapterRuntimeError> {
    if text.len() > cap || text.contains('\0') {
        return Err(AdapterRuntimeError::BatchBytes);
    }
    Ok(text.to_owned())
}

/// Display text: a long description is cut at a character boundary rather
/// than costing the whole list.
fn clipped(text: &str, cap: usize) -> Result<String, AdapterRuntimeError> {
    let end = (0..=cap.min(text.len()))
        .rev()
        .find(|end| text.is_char_boundary(*end))
        .unwrap_or(0);
    bounded(&text[..end], cap)
}

/// ACP `availableCommands`, in advertised order.
pub(crate) fn parse_acp(value: &Value) -> Result<Vec<AvailableCommand>, AdapterRuntimeError> {
    value
        .as_array()
        .filter(|values| values.len() <= MAX_COMMANDS)
        .ok_or(AdapterRuntimeError::BatchCount)?
        .iter()
        .map(AvailableCommand::from_acp)
        .collect()
}

/// Admits a whole list: bounded in count and bytes, names unique.
fn admit(commands: Vec<AvailableCommand>) -> Result<Vec<AvailableCommand>, AdapterRuntimeError> {
    if commands.len() > MAX_COMMANDS {
        return Err(AdapterRuntimeError::BatchCount);
    }
    let mut bytes = 0;
    for (index, command) in commands.iter().enumerate() {
        if commands[..index].iter().any(|c| c.name == command.name) {
            return Err(AdapterRuntimeError::Frame);
        }
        bytes += command.name.len()
            + command.description.len()
            + command.input_hint.as_ref().map_or(0, String::len);
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
    /// Takes a binding's newly parsed list. A failed parse still counts as a
    /// new revision and leaves the list empty, so stale command authority is
    /// never usable.
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
    /// The listed command a `/name …` prompt runs at this revision.
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
