//! One advertised command: its name, wording, source and native route,
//! with the bounds every entry shares.
use super::MAX_COMMANDS;
use crate::acp::bindings::adapter::AdapterRuntimeError;
use serde::Serialize;
use serde_json::Value;

const NAME_BYTES: usize = 128;
const DESCRIPTION_BYTES: usize = 1024;
const HINT_BYTES: usize = 256;
/// Other names a harness runs a command by; more are left out.
const MAX_ALIASES: usize = 8;

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

impl CommandSource {
    /// A source as harnesses name it on the wire; anything else is the
    /// harness's own, unclassified.
    pub(crate) fn from_wire(value: Option<&str>) -> Self {
        match value {
            Some("builtin") => Self::Builtin,
            Some("skill") => Self::Skill,
            Some("prompt") => Self::Prompt,
            Some("extension") => Self::Extension,
            Some("mcp") => Self::Mcp,
            Some("plugin") => Self::Plugin,
            _ => Self::Harness,
        }
    }
}

/// The listed command a Turn ran, kept beside its prompt so the chat shows
/// it as a command after a reload. The list itself is never stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct CommandMark {
    pub(crate) name: String,
    pub(crate) source: CommandSource,
}

/// How the binding that advertised a command runs it. Native only: the
/// WebView names a command and the binding looks its dispatch up in the
/// live list, so a prompt can never choose its own route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CommandDispatch {
    /// Sent as `/name input` prompt text, which the harness parses.
    Prompt,
    /// Sent as prompt text that the harness runs as a command turn, which
    /// may finish without model work (Pi's extension commands).
    CommandTurn,
    /// Run by the binding's own method for this name (Pi's `compact`).
    Builtin,
    /// Sent with the skill attached by its harness path (Codex skills).
    Skill { path: String },
    /// Run by the host as the session action its name and input name, with
    /// the action's capability gate (Codex's `compact` and `review`).
    Action,
    /// Opens a panel in the chat and sends nothing (Codex's `status` and
    /// `mcp`). The WebView learns the panel from the list, since it opens
    /// the panel without running the command.
    Panel(CommandPanel),
}

/// A panel a command opens in the chat, named on the list it came with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CommandPanel {
    /// The session's model, effort, access and usage.
    Status,
    /// The session's MCP servers.
    Mcp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AvailableCommand {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) input_hint: Option<String>,
    pub(crate) source: CommandSource,
    #[serde(
        rename = "panel",
        serialize_with = "serialize_panel",
        skip_serializing_if = "opens_no_panel"
    )]
    pub(crate) dispatch: CommandDispatch,
    /// Other names the harness runs this command by, which the menu matches
    /// too. They give no route of their own: a prompt still names the
    /// listed command to run as one.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) aliases: Vec<String>,
}

fn opens_no_panel(dispatch: &CommandDispatch) -> bool {
    !matches!(dispatch, CommandDispatch::Panel(_))
}

/// Only a panel is named; every other route stays native.
fn serialize_panel<S: serde::Serializer>(
    dispatch: &CommandDispatch,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match dispatch {
        CommandDispatch::Panel(panel) => panel.serialize(serializer),
        _ => serializer.serialize_none(),
    }
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
        if !is_command_name(name) {
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
            aliases: Vec::new(),
        })
    }

    /// The command with the other names its harness runs it by. An alias
    /// that is not a command name, repeats one, or is past the bound is left
    /// out, since aliases only help the menu find the command.
    pub(crate) fn with_aliases<'a>(mut self, aliases: impl IntoIterator<Item = &'a str>) -> Self {
        for alias in aliases {
            if self.aliases.len() == MAX_ALIASES {
                break;
            }
            if is_command_name(alias)
                && alias.len() <= NAME_BYTES
                && alias != self.name
                && !self.aliases.iter().any(|known| known == alias)
            {
                self.aliases.push(alias.to_owned());
            }
        }
        self
    }

    /// What this command counts toward a list's `MAX_COMMAND_BYTES`,
    /// including the native route it keeps, such as a skill's path.
    pub(crate) fn listed_bytes(&self) -> usize {
        let route = match &self.dispatch {
            CommandDispatch::Skill { path } => path.len(),
            _ => 0,
        };
        self.name.len()
            + self.description.len()
            + self.input_hint.as_ref().map_or(0, String::len)
            + self.aliases.iter().map(String::len).sum::<usize>()
            + route
    }

    /// An ACP `AvailableCommand`. ACP has no source or alias fields, so an
    /// adapter may name them in `_meta.source` and `_meta.aliases`; a
    /// command with no source is the harness's own.
    pub(crate) fn from_acp(value: &Value) -> Result<Self, AdapterRuntimeError> {
        let source = CommandSource::from_wire(value["_meta"]["source"].as_str());
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
        .map(|command| {
            let aliases = value["_meta"]["aliases"]
                .as_array()
                .map_or(&[][..], Vec::as_slice);
            command.with_aliases(aliases.iter().filter_map(Value::as_str))
        })
    }
}

/// A name a prompt can run as `/name`: no whitespace, controls or slash.
fn is_command_name(name: &str) -> bool {
    !name.is_empty()
        && !name
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '/')
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
