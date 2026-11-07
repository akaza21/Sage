//! One pending bounded edit; all path reads use the existing held-root walker.
use crate::workspace::{Workspace, open_workspace_text_file};
use serde::Deserialize;
use std::{io::Read, path::Path};

pub(crate) const MAX_BYTES: usize = 262_144;
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

pub(crate) fn prepare(workspace: &Workspace, request: Request) -> Result<Edit, ()> {
    if request.version != 1 || request.tool_call_id.is_empty() || request.tool_call_id.len() > 500 {
        return Err(());
    }
    let args = request.args.as_object().ok_or(())?;
    let text = |key| args.get(key).and_then(serde_json::Value::as_str).ok_or(());
    let relative = super::path::admit_requested(workspace, text("path")?)?;
    // Reject symlinks/reparse points rather than giving an alias write authority.
    let old = read(workspace, &relative)?;
    super::path::admit_resolved(workspace, &relative)?;
    let created = old.is_none();
    let old = old.unwrap_or_default();
    let new = match request.tool.as_str() {
        "write" if args.len() == 2 => text("content")?.to_owned(),
        "edit" if args.len() == 3 && !created => {
            let before = text("oldText")?;
            if before.is_empty() || old.matches(before).count() != 1 {
                return Err(());
            }
            old.replacen(before, text("newText")?, 1)
        }
        _ => return Err(()), // Shell is never granted filesystem authority.
    };
    if new.len() > MAX_BYTES || old.contains('\0') || new.contains('\0') {
        return Err(());
    }
    Ok(Edit {
        tool_id: request.tool_call_id,
        relative,
        old,
        new,
        created,
    })
}

pub(crate) fn read(workspace: &Workspace, relative: &str) -> Result<Option<String>, ()> {
    let path = workspace.root().join(relative);
    let parent = Path::new(relative)
        .parent()
        .and_then(Path::to_str)
        .ok_or(())?;
    let _held =
        crate::workspace::open_held_parent(workspace.root(), workspace.root_identity(), parent)
            .map_err(|_| ())?;
    match std::fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => {}
        _ => return Err(()),
    }
    let file =
        open_workspace_text_file(workspace.root(), workspace.root_identity(), relative, None)
            .map_err(|_| ())?;
    let mut bytes = Vec::with_capacity(MAX_BYTES + 1);
    file.take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ())?;
    if bytes.len() > MAX_BYTES {
        return Err(());
    }
    String::from_utf8(bytes).map(Some).map_err(|_| ())
}
