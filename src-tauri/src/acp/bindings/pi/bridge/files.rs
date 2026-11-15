use super::Refusal;
use crate::workspace::{Workspace, open_workspace_text_file};
use serde::Deserialize;
use std::{io::Read, path::Path};

pub(crate) const MAX_BYTES: usize = 262_144;
pub(crate) const MAX_COMMAND_BYTES: usize = 16_384;
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct Request {
    pub version: u8,
    pub tool_call_id: String,
    pub tool: String,
    pub args: serde_json::Value,
}

pub(crate) struct Edit {
    pub tool_id: String,
    pub relative: String,
    pub old: String,
    pub new: String,
    pub created: bool,
}

/// Sage holds no shell authority: approval is the only thing it grants.
pub(crate) struct Shell {
    pub tool_id: String,
    /// The card's text: the command on one line with credentials redacted.
    pub shown: String,
}

pub(crate) enum Prepared {
    Edit(Edit),
    Shell(Shell),
}

impl Prepared {
    pub(crate) fn tool_id(&self) -> &str {
        match self {
            Self::Edit(edit) => &edit.tool_id,
            Self::Shell(shell) => &shell.tool_id,
        }
    }
}

pub(crate) fn prepare(workspace: &Workspace, request: Request) -> Result<Prepared, Refusal> {
    if request.version != 1 || request.tool_call_id.is_empty() || request.tool_call_id.len() > 500 {
        return Err(Refusal::Unsupported);
    }
    if request.tool == "bash" {
        return shell(request).map(Prepared::Shell);
    }
    prepare_edit(workspace, request).map(Prepared::Edit)
}

/// Pi's bash takes a command and an optional timeout in seconds.
fn shell(request: Request) -> Result<Shell, Refusal> {
    let args = request.args.as_object().ok_or(Refusal::Unsupported)?;
    if args.keys().any(|key| key != "command" && key != "timeout")
        || args
            .get("timeout")
            .is_some_and(|timeout| !timeout.as_f64().is_some_and(|t| t > 0.0))
    {
        return Err(Refusal::Unsupported);
    }
    let command = args
        .get("command")
        .and_then(serde_json::Value::as_str)
        .filter(|command| !command.trim().is_empty())
        .ok_or(Refusal::Unsupported)?;
    if command.len() > MAX_COMMAND_BYTES {
        return Err(Refusal::TooLarge);
    }
    // The card must show what runs: no invisible or reordering characters,
    // and no blank lines that push the rest of a command out of view.
    if command
        .chars()
        .any(crate::acp::bindings::command_summary::is_hidden_format)
        || command.lines().any(|line| line.trim().is_empty())
    {
        return Err(Refusal::Undisplayable);
    }
    let shown = crate::acp::bindings::command_summary::sanitize_command_summary(command)
        .map_err(|_| Refusal::Undisplayable)?;
    Ok(Shell {
        tool_id: request.tool_call_id,
        shown: shown.as_str().to_owned(),
    })
}

fn prepare_edit(workspace: &Workspace, request: Request) -> Result<Edit, Refusal> {
    let args = request.args.as_object().ok_or(Refusal::Unsupported)?;
    let text = |key| {
        args.get(key)
            .and_then(serde_json::Value::as_str)
            .ok_or(Refusal::Unsupported)
    };
    let replaces = match (request.tool.as_str(), args.len()) {
        ("write", 2) => false,
        ("edit", 3) => true,
        _ => return Err(Refusal::Unsupported),
    };
    let relative = super::path::admit_requested(workspace, text("path")?)?;
    // Reject symlinks/reparse points rather than giving an alias write authority.
    let old = read(workspace, &relative)?;
    super::path::admit_resolved(workspace, &relative)?;
    let created = old.is_none();
    let old = old.unwrap_or_default();
    let new = if replaces {
        let before = text("oldText")?;
        if created || before.is_empty() || old.matches(before).count() != 1 {
            return Err(Refusal::NoUniqueMatch);
        }
        old.replacen(before, text("newText")?, 1)
    } else {
        text("content")?.to_owned()
    };
    if new.len() > MAX_BYTES {
        return Err(Refusal::TooLarge);
    }
    if old.contains('\0') || new.contains('\0') {
        return Err(Refusal::Unreadable);
    }
    Ok(Edit {
        tool_id: request.tool_call_id,
        relative,
        old,
        new,
        created,
    })
}

pub(crate) fn read(workspace: &Workspace, relative: &str) -> Result<Option<String>, Refusal> {
    let path = workspace.root().join(relative);
    let parent = Path::new(relative)
        .parent()
        .and_then(Path::to_str)
        .ok_or(Refusal::OutsideWorkspace)?;
    let _held =
        crate::workspace::open_held_parent(workspace.root(), workspace.root_identity(), parent)
            .map_err(|_| Refusal::MissingParent)?;
    match std::fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => {}
        _ => return Err(Refusal::Unreadable),
    }
    let file =
        open_workspace_text_file(workspace.root(), workspace.root_identity(), relative, None)
            .map_err(|_| Refusal::Unreadable)?;
    let mut bytes = Vec::with_capacity(MAX_BYTES + 1);
    file.take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Refusal::Unreadable)?;
    if bytes.len() > MAX_BYTES {
        return Err(Refusal::TooLarge);
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| Refusal::Unreadable)
}
