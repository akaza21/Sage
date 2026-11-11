//! The native Claude installer links its launcher to a versioned executable.
//! Only that layout is allowed. Admission still pins and revalidates the file.

use std::path::Path;

use super::validation::{authentic_regular_file, is_local_probe_root};

pub(super) fn authentic_launcher(path: &Path) -> bool {
    native_target(path).is_some()
}

fn native_target(path: &Path) -> Option<()> {
    if path.file_name()? != "claude" {
        return None;
    }
    let bin = path.parent()?;
    let local = bin.parent()?;
    if bin.file_name()? != "bin" || local.file_name()? != ".local" {
        return None;
    }
    let versions = local.join("share/claude/versions");
    let link = std::fs::read_link(path).ok()?;
    let version = link.file_name()?.to_str()?;
    let target = versions.join(version);
    let relative = Path::new("../share/claude/versions").join(version);
    if (link != target && link != relative)
        || version.len() > 32
        || version.split('.').count() != 3
        || !version
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
        || !plain_ancestors(bin)
        || !plain_ancestors(&versions)
        || !authentic_regular_file(&target)
    {
        return None;
    }
    Some(())
}

/// Bound filesystem checks and reject symlinks in the launcher and store's parents.
fn plain_ancestors(path: &Path) -> bool {
    let mut ancestors = path.ancestors();
    for _ in 0..128 {
        let Some(parent) = ancestors.next() else {
            return true;
        };
        if !is_local_probe_root(parent)
            || !std::fs::symlink_metadata(parent)
                .is_ok_and(|meta| meta.is_dir() && !meta.file_type().is_symlink())
        {
            return false;
        }
    }
    ancestors.next().is_none()
}
