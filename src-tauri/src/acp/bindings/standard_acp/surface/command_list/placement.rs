//! Curated placement decorates reported commands; it never invents a catalog or a route.
use crate::acp::{
    bindings::release_version::ReleaseVersion,
    slash_commands::{AvailableCommand, CommandSource},
};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

const MAX_ENTRIES: usize = 128;
const MAX_TABLE_BYTES: usize = 64 * 1024;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CommandSection {
    AddToMessage,
    Session,
    Connections,
    AddOns,
    SkillsAndCommands,
    ModelAndAccess,
    Usage,
    #[default]
    More,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum NativeControl {
    GoalEditor,
    Compact,
    ModelPicker,
    EffortPicker,
    McpPanel,
    UsagePanel,
    Review,
    NewChat,
    AccessMenu,
    ReloadAddOns,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CommandPlacement {
    pub(crate) section: CommandSection,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) control: Option<NativeControl>,
    #[serde(skip)]
    pub(crate) known: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct KnownCommand {
    name: String,
    #[serde(default)]
    aliases: Vec<String>,
    section: CommandSection,
    control: Option<NativeControl>,
    since: Option<String>,
    terminal_only: Option<bool>,
}

/// Installation metadata only, loaded once per curated harness. Invalid data fails closed.
pub(crate) struct KnownCommandTable(Vec<KnownCommand>);

impl KnownCommandTable {
    pub(crate) fn load(cache: &'static OnceLock<Self>, bytes: &[u8]) -> &'static Self {
        cache.get_or_init(|| Self::parse(bytes).expect("invalid curated command metadata"))
    }

    fn parse(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.len() > MAX_TABLE_BYTES {
            return Err("command metadata exceeds byte cap");
        }
        let entries: Vec<KnownCommand> =
            serde_json::from_slice(bytes).map_err(|_| "malformed command metadata")?;
        if entries.len() > MAX_ENTRIES {
            return Err("command metadata exceeds entry cap");
        }
        let valid_name = |name: &str| {
            !name.is_empty()
                && name.len() <= 128
                && !name
                    .chars()
                    .any(|c| c.is_whitespace() || c.is_control() || c == '/' || c == '\u{feff}')
        };
        let mut names = std::collections::HashSet::new();
        for entry in &entries {
            if entry.aliases.len() > 8
                || entry.terminal_only == Some(false)
                || entry.since.as_ref().is_some_and(|version| {
                    version.len() > 96
                        || version.split_ascii_whitespace().count() != 1
                        || ReleaseVersion::parse(version).is_none()
                })
                || std::iter::once(&entry.name)
                    .chain(&entry.aliases)
                    .any(|name| !valid_name(name) || !names.insert(name.as_str()))
            {
                return Err("invalid or ambiguous command metadata");
            }
        }
        Ok(Self(entries))
    }
}

/// Minimum versions gate native controls, never the harness's advertised command.
/// User skills/plugins cannot acquire native command authority.
pub(crate) fn placement(
    command: &AvailableCommand,
    table: &KnownCommandTable,
    running: Option<&ReleaseVersion>,
    supports: &dyn Fn(NativeControl) -> bool,
) -> CommandPlacement {
    let known = matches!(
        command.source,
        CommandSource::Builtin | CommandSource::Harness
    )
    .then(|| {
        table.0.iter().find(|entry| {
            std::iter::once(&entry.name)
                .chain(&entry.aliases)
                .any(|name| name == &command.name || command.aliases.contains(name))
        })
    })
    .flatten();
    let Some(known) = known else {
        return CommandPlacement::default();
    };
    let version_supported = known
        .since
        .as_deref()
        .and_then(ReleaseVersion::parse)
        .is_none_or(|minimum| running.is_some_and(|running| running >= &minimum));
    CommandPlacement {
        section: known.section,
        control: known
            .control
            .filter(|control| version_supported && supports(*control)),
        known: true,
    }
}

/// Shared by catalog admission and dependency changes. Works on the existing bounded
/// catalog without retaining another copy; unchanged placement preserves its revision.
pub(crate) fn recompute_placement(
    commands: &mut [AvailableCommand],
    table: &KnownCommandTable,
    running: Option<&ReleaseVersion>,
    supports: &dyn Fn(NativeControl) -> bool,
) -> bool {
    let mut changed = false;
    for command in commands {
        let next = placement(command, table, running, supports);
        changed |= command.placement != next;
        command.placement = next;
    }
    changed
}

pub(crate) fn place_commands(
    mut commands: Vec<AvailableCommand>,
    table: &KnownCommandTable,
    running: Option<&ReleaseVersion>,
    supports: &dyn Fn(NativeControl) -> bool,
) -> Vec<AvailableCommand> {
    recompute_placement(&mut commands, table, running, supports);
    commands
}

#[cfg(test)]
mod tests;
