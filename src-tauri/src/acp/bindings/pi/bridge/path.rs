//! Write admission for one workspace-relative path. A name is refused when it
//! could alias a protected directory on any supported file system, or when the
//! approval prompt could not show it to the user exactly as it will be written.
use crate::workspace::{
    Workspace,
    directory_skip::{protected_workspace_name, windows_reserved_device_name},
};
use std::path::{Component, Path};

/// The approval prompt names the path in full, so it must fit the summary.
pub(crate) const MAX_DISPLAY_PATH_BYTES: usize = 480;

/// Normalises a requested path to `/`-separated relative form and admits it.
pub(crate) fn admit_requested(workspace: &Workspace, requested: &str) -> Result<String, ()> {
    if !cfg!(windows) && requested.contains('\\') {
        return Err(()); // A literal backslash would name a different file on Unix.
    }
    let path = Path::new(requested);
    let relative = if path.is_absolute() {
        path.strip_prefix(workspace.root()).map_err(|_| ())?
    } else {
        path
    };
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(());
    }
    let relative = relative.to_str().ok_or(())?.replace('\\', "/");
    if relative.len() > MAX_DISPLAY_PATH_BYTES || !relative.split('/').all(admitted_name) {
        return Err(());
    }
    Ok(relative)
}

/// Re-decides against the real location: the existing parent, and the target
/// when it exists, resolved through links, case and Windows short names.
pub(crate) fn admit_resolved(workspace: &Workspace, relative: &str) -> Result<(), ()> {
    let root = workspace.root().canonicalize().map_err(|_| ())?;
    let target = workspace.root().join(relative);
    let parent = target.parent().ok_or(())?.canonicalize().map_err(|_| ())?;
    let resolved = match target.canonicalize() {
        Ok(existing) => existing,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            parent.join(target.file_name().ok_or(())?)
        }
        Err(_) => return Err(()),
    };
    if resolved.parent() != Some(parent.as_path()) {
        return Err(());
    }
    let inside = resolved.strip_prefix(&root).map_err(|_| ())?;
    if inside.as_os_str().is_empty() {
        return Err(());
    }
    inside
        .components()
        .all(|part| match part {
            Component::Normal(name) => name.to_str().is_some_and(admitted_name),
            _ => false,
        })
        .then_some(())
        .ok_or(())
}

fn admitted_name(name: &str) -> bool {
    !name.is_empty()
        && !protected_workspace_name(name)
        && !windows_reserved_device_name(name)
        && !name.ends_with(['.', ' '])
        && !name.contains(':')
        && !short_name(name)
        && !name.chars().any(hidden_char)
}

/// Windows 8.3 aliases such as `GIT~1` or `NODE_M~2.JS` can name a protected
/// directory without spelling it.
fn short_name(name: &str) -> bool {
    let stem = name.split_once('.').map_or(name, |(stem, _)| stem);
    stem.rsplit_once('~').is_some_and(|(_, digits)| {
        !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
    })
}

/// Characters that would hide or reorder the path in the approval prompt.
fn hidden_char(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{00ad}'
                | '\u{061c}'
                | '\u{180e}'
                | '\u{200b}'..='\u{200f}'
                | '\u{2028}'..='\u{202e}'
                | '\u{2060}'..='\u{206f}'
                | '\u{feff}'
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protected_and_ambiguous_names_are_refused() {
        for name in [
            ".git",
            ".GIT",
            ".Git",
            "Node_Modules",
            "GIT~1",
            "NODE_M~2.JS",
            "config.",
            "config ",
            ".git.",
            "a:stream",
            "line\nbreak",
            "safe\u{202e}txt.exe",
            "zero\u{200b}width",
            "nul",
            "CON",
            "Prn",
            "aux.json",
            "con.txt",
            "con.tar.gz",
            "nul .txt",
            "COM1",
            "com9.log",
            "lpt1",
            "LPT9.txt",
            "com\u{b9}",
            "lpt\u{b2}.txt",
            "COM\u{b3}",
        ] {
            assert!(!admitted_name(name), "{name:?}");
        }
        for name in [
            "src",
            "notes~",
            "a~b.txt",
            "v1.2~draft",
            "README.md",
            "console.txt",
            "nul_device",
            "com10",
            "lpt",
            "aux-tools",
            "my.con",
        ] {
            assert!(admitted_name(name), "{name:?}");
        }
    }
}
