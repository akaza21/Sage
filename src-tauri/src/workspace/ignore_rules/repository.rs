use std::path::{Component, Path, PathBuf};

use ignore::gitignore::Gitignore;

use super::{IgnoreRules, Skipped, matcher, present, read_rules, unresolved};

/// Only the root's own `.git` file (a worktree opened as the Project) may lead
/// outside the root.
pub(super) fn repository_exclude(
    rules: &IgnoreRules,
    directory: &Path,
    is_root: bool,
) -> Result<Option<Gitignore>, Skipped> {
    let root = &rules.root;
    let dot_git = directory.join(".git");
    let Some(metadata) = present(&dot_git, dot_git.symlink_metadata())? else {
        return Ok(None);
    };
    let mut git_directory = if metadata.is_dir() {
        dot_git
    } else {
        let Some(link) = read_rules(&dot_git, false)? else {
            return Ok(None);
        };
        String::from_utf8(link)
            .ok()
            .and_then(|link| {
                let target = link.trim().strip_prefix("gitdir:")?;
                git_target(directory, target, root, is_root)
            })
            .ok_or_else(|| unresolved(&dot_git))?
    };
    let commondir = git_directory.join("commondir");
    if let Some(common) = read_rules(&commondir, true)? {
        git_directory = String::from_utf8(common)
            .ok()
            .and_then(|common| git_target(&git_directory, &common, root, is_root))
            .ok_or_else(|| unresolved(&commondir))?;
    }
    Ok(read_rules(&git_directory.join("info").join("exclude"), true)?.and_then(|b| matcher(&b)))
}

/// Where a `gitdir:` or `commondir` value written in `base` leads, resolved
/// without touching the file system. Network paths (`\\host\share`,
/// `//host`) are refused, and unless `outside_allowed` the result must stay
/// inside `root`, so a `.git` file in a cloned or unzipped folder cannot make
/// a listing contact another machine (an automounted `/net/host`, say).
pub(super) fn git_target(
    base: &Path,
    text: &str,
    root: &Path,
    outside_allowed: bool,
) -> Option<PathBuf> {
    let text = text.trim();
    if text.is_empty() || text.starts_with("\\\\") || text.starts_with("//") {
        return None;
    }
    let path = Path::new(text);
    let relative = path.components().all(|component| {
        matches!(
            component,
            Component::Normal(_) | Component::CurDir | Component::ParentDir
        )
    });
    if !relative && !path.is_absolute() {
        return None;
    }
    #[cfg(windows)]
    if !relative
        && !matches!(
            path.components().next(),
            Some(Component::Prefix(prefix)) if matches!(prefix.kind(), std::path::Prefix::Disk(_))
        )
    {
        return None;
    }
    let mut target = PathBuf::new();
    for component in base.join(path).components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                target.pop();
            }
            other => target.push(other),
        }
    }
    (outside_allowed || target.starts_with(root)).then_some(target)
}
