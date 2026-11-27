//! Curated placement decorates reported commands; it never invents a catalog or a route.
use crate::acp::{
    bindings::release_version::ReleaseVersion,
    slash_commands::{AvailableCommand, CommandSource},
};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

const MAX_ENTRIES: usize = 128;
const MAX_TABLE_BYTES: usize = 64 * 1024;

mod terminal;
pub(crate) use terminal::{TerminalHandoff, TerminalMetadata};

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
    /// The release that retired the command; only a draft reads it, since a
    /// live list leaves a retired command out itself.
    removed: Option<String>,
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
                || [&entry.since, &entry.removed]
                    .into_iter()
                    .flatten()
                    .any(|version| {
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

    /// Known commands whose curated `since` is newer than `installed` and no
    /// newer than `latest`: what that upgrade unlocks. Entries without `since`
    /// are never listed, so nothing is guessed.
    pub(crate) fn introduced_between<'a>(
        &'a self,
        installed: &'a ReleaseVersion,
        latest: &'a ReleaseVersion,
    ) -> impl Iterator<Item = &'a str> + 'a {
        self.0.iter().filter_map(move |entry| {
            let since = ReleaseVersion::parse(entry.since.as_deref()?)?;
            (&since > installed && &since <= latest).then_some(entry.name.as_str())
        })
    }

    /// The curated built-in commands a new-chat draft offers before any
    /// session exists: neither terminal-only nor a native control, and
    /// introduced no later than `installed`. A `since` entry is left out when
    /// the installed release is unknown, so nothing is guessed. At most
    /// [`MAX_DRAFT_BUILTINS`].
    pub(crate) fn draft_builtins<'a>(
        &'a self,
        installed: Option<&'a ReleaseVersion>,
    ) -> impl Iterator<Item = &'a str> + 'a {
        self.0
            .iter()
            .filter(move |entry| {
                entry.terminal_only.is_none()
                    && entry.control.is_none()
                    && entry.since.as_deref().is_none_or(|since| {
                        ReleaseVersion::parse(since)
                            .zip(installed)
                            .is_some_and(|(since, installed)| installed >= &since)
                    })
                    && entry.removed.as_deref().is_none_or(|removed| {
                        ReleaseVersion::parse(removed)
                            .zip(installed)
                            .is_some_and(|(removed, installed)| installed < &removed)
                    })
            })
            .map(|entry| entry.name.as_str())
            .take(MAX_DRAFT_BUILTINS)
    }

    /// Whether the curated entry for `name` (or an alias) is a native control
    /// or terminal-only: it needs a started chat, so a draft leaves it out.
    pub(crate) fn needs_session(&self, name: &str) -> bool {
        self.0.iter().any(|entry| {
            (entry.name == name || entry.aliases.iter().any(|alias| alias == name))
                && (entry.control.is_some() || entry.terminal_only.is_some())
        })
    }
}

/// The most built-in commands a draft lists; the curated tables hold far fewer.
pub(crate) const MAX_DRAFT_BUILTINS: usize = 32;

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
