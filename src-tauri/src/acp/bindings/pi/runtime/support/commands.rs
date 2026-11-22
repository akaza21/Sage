use serde_json::Value;

use crate::acp::{
    bindings::{
        adapter::{AdapterEgress, AdapterEvent, AdapterRuntimeError},
        standard_acp::surface::command_list::{
            AvailableCommand, CommandDispatch, CommandPanel, CommandSource, CommandSourceInfo,
            wire_text,
        },
    },
    command_placement::{NativeControl, place_commands},
    session_action::SessionAction,
    transcript_quote::Prompt,
};

use super::super::{PiRuntime, RequestKind};
use crate::acp::bindings::pi::Operation;

/// Pi's built-in commands take precedence over templates or skills of the same name. RPC does
/// not list them, so their wording comes from Pi's command table.
const COMPACT: &str = "compact";
const SESSION: &str = "session";

/// One entry of Pi's `get_commands`. Its `source` says what the command
/// is; only extension commands run as command turns. An extension Pi ships
/// itself (`sourceInfo.source: "builtin"`, such as `builtin:mcp`) is listed as
/// built in, so curated placement may give it a native control, while it still
/// runs as Pi's own command turn.
fn command(value: &Value) -> Result<AvailableCommand, AdapterRuntimeError> {
    let wire = CommandSource::from_wire(value["source"].as_str());
    let dispatch = match wire {
        CommandSource::Extension => CommandDispatch::CommandTurn,
        _ => CommandDispatch::Prompt,
    };
    let source = if wire == CommandSource::Extension && value["sourceInfo"]["source"] == "builtin" {
        CommandSource::Builtin
    } else {
        wire
    };
    let description = match value.get("description") {
        None => "",
        Some(description) => wire_text(description)?,
    };
    AvailableCommand::new(
        wire_text(&value["name"])?,
        description,
        None,
        source,
        dispatch,
    )
    .map(|command| {
        let mut command = command.with_source_label(source_label(value));
        command.source_info = CommandSourceInfo::from_wire(&value["sourceInfo"]).map(Box::new);
        command
    })
}

/// Pi reports package roots and extension paths, rather than a display label.
fn source_label(value: &Value) -> Option<&str> {
    let info = &value["sourceInfo"];
    fn leaf(path: &str) -> Option<&str> {
        path.trim_end_matches(['/', '\\'])
            .rsplit(['/', '\\'])
            .next()
            .filter(|name| !name.is_empty())
    }
    if info["origin"] == "package"
        && let Some(package) = info["baseDir"].as_str().and_then(leaf)
    {
        return Some(package);
    }
    let file = info["path"].as_str().and_then(leaf)?;
    Some(file.rsplit_once('.').map_or(file, |(stem, _)| stem))
}

/// A catalog larger than Sage's list keeps its first commands, leaving room for the built-ins.
fn with_builtins(value: &Value) -> Result<Vec<AvailableCommand>, AdapterRuntimeError> {
    let builtins = [
        AvailableCommand::new(
            SESSION,
            "Show session info and stats",
            None,
            CommandSource::Builtin,
            CommandDispatch::Panel(CommandPanel::Status),
        )?,
        AvailableCommand::new(
            COMPACT,
            "Manually compact the session context",
            None,
            CommandSource::Builtin,
            CommandDispatch::Builtin,
        )?,
    ];
    let listed = value.as_array().ok_or(AdapterRuntimeError::Frame)?;
    let commands = builtins
        .into_iter()
        .map(Ok)
        .chain(listed.iter().map(command))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(commands)
}

impl PiRuntime {
    pub(super) fn list_commands(
        &mut self,
        value: Value,
    ) -> Result<AdapterEvent, AdapterRuntimeError> {
        self.commands_pending = false;
        let version = self
            .launch
            .as_ref()
            .and_then(|launch| launch.harness_version())
            .and_then(crate::acp::bindings::release_version::ReleaseVersion::parse);
        let supports = |control| match control {
            NativeControl::Compact | NativeControl::UsagePanel | NativeControl::McpPanel => {
                self.initialized
            }
            NativeControl::ModelPicker => self.initialized && !self.available_models.is_empty(),
            _ => false,
        };
        self.commands
            .replace(with_builtins(&value).map(|commands| {
                place_commands(
                    commands,
                    crate::acp::bindings::pi::manifest::known_commands(),
                    version.as_ref(),
                    &supports,
                )
            }))?;
        Ok(AdapterEvent::Absorbed)
    }

    pub(in crate::acp::bindings::pi::runtime) fn begin_prompt(
        &mut self,
        prompt: &Prompt,
        turn_key: &str,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        let listed = self.commands.snapshot();
        let text = prompt.as_str();
        let found = |revision: u64| {
            let listed = listed.find(revision, text)?;
            let rest = text[1..].trim_start().strip_prefix(&listed.name)?.trim();
            Some((listed.dispatch.clone(), listed.name.as_str(), rest))
        };
        // Pi runs a listed extension command by name whatever revision the
        // prompt saw, so a stale pick still runs as a command turn.
        let command = prompt.command_revision.and_then(&found).or_else(|| {
            found(listed.revision)
                .filter(|(dispatch, ..)| matches!(dispatch, CommandDispatch::CommandTurn))
        });
        let command_turn =
            prompt.action.is_none() && matches!(command, Some((CommandDispatch::CommandTurn, ..)));
        let turn_key = turn_key.to_owned();
        let operation = match (&prompt.action, command) {
            (Some(SessionAction::Compact {}), _) => Operation::Compact {
                turn_key,
                instructions: None,
            },
            (Some(SessionAction::Review { .. }), _) => {
                return Err(AdapterRuntimeError::UnsupportedCommand);
            }
            (None, Some((CommandDispatch::Builtin, COMPACT, instructions))) => Operation::Compact {
                turn_key,
                instructions: (!instructions.is_empty()).then(|| instructions.to_owned()),
            },
            (None, _) => Operation::Prompt {
                turn_key,
                message: prompt.resources.inline(prompt.as_str()),
                // Pi repeats the images in its own events, so they take the
                // echoed bound.
                images: prompt
                    .images
                    .echoed()
                    .map_err(|_| AdapterRuntimeError::UnsupportedCommand)?
                    .into_iter()
                    .map(|(media_type, data)| {
                        serde_json::json!({"type": "image", "data": data, "mimeType": media_type})
                    })
                    .collect(),
            },
        };
        let egress = self.begin_one(operation, RequestKind::Prompt)?;
        if command_turn {
            self.translator.mark_command_turn().map_err(super::map_pi)?;
            // An extension command may change the model without an event;
            // the next prompt reads the configuration again.
            self.configuration_dirty = true;
        }
        Ok(egress)
    }
}
