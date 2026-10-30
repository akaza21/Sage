//! Keeps an index current from file events without walking the root again.
//! Each changed path is looked at once when Quick Open next queries: its old
//! entries go, and whatever exists there now comes back in, under the same
//! rules as a full build.

use std::{ffi::OsStr, fs, path::Path};

use crate::workspace::{
    directory_skip::skip_workspace_directory_name,
    ignore_rules::{IgnoreRules, RuleFiles},
    path_glob::PathGlobs,
};

use super::{
    arena::PathArena,
    build::{Folder, Walk},
};

/// More changed paths than this at once (a branch switch, a build) cost
/// more to reconcile one by one than to walk the root again.
pub(super) const MAX_REFRESH_PATHS: usize = 64;

/// What a batch of changed paths needs.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Refresh {
    Applied,
    /// Too many paths, or an ignore file changed: walk the root again.
    Rebuild,
}

/// Whether a changed path can matter to Quick Open. Paths under `.git` or
/// `node_modules` never are.
pub(super) fn relevant(relative: &str) -> bool {
    !relative
        .split('/')
        .any(|part| skip_workspace_directory_name(OsStr::new(part)))
}

pub(super) fn apply(
    root: &Path,
    arena: &mut PathArena,
    changed: &[String],
    walk: &Walk<'_>,
) -> Refresh {
    if changed.len() > MAX_REFRESH_PATHS
        || changed.iter().any(|path| {
            let name = path.rsplit('/').next().unwrap_or(path);
            name == ".gitignore" || name == ".ignore"
        })
    {
        return Refresh::Rebuild;
    }
    let prefixes = changed.iter().map(String::as_str).collect::<Vec<_>>();
    arena.remove_under(&prefixes);
    let mut folders = Vec::new();
    let mut files = Vec::new();
    for relative in changed {
        let Ok(metadata) = fs::symlink_metadata(root.join(relative)) else {
            continue;
        };
        let is_dir = metadata.is_dir();
        if !(is_dir || metadata.is_file()) {
            continue;
        }
        let Some(chain) = admitted(root, walk.rules, walk.excluded, relative, is_dir) else {
            continue;
        };
        if is_dir {
            folders.push(Folder {
                path: root.join(relative),
                depth: relative.split('/').count(),
                relative: relative.clone(),
                chain,
            });
        } else {
            files.push(relative.as_str());
        }
    }
    // A path reported twice, or inside a reported folder, comes back once.
    files.retain(|file| {
        !folders
            .iter()
            .any(|folder| file.starts_with(&format!("{}/", folder.relative)))
    });
    files.sort_unstable();
    files.dedup();
    let mut room = walk.room;
    for file in files {
        if room == 0 {
            return Refresh::Rebuild;
        }
        if arena.push(file) {
            room -= 1;
        }
    }
    let folder_walk = Walk {
        room,
        threads: 1,
        ..*walk
    };
    if folder_walk.run(folders, arena) {
        return Refresh::Rebuild;
    }
    Refresh::Applied
}

/// The ignore chain of the folder holding `relative`, when neither it nor any
/// folder above it is excluded or ignored.
fn admitted(
    root: &Path,
    rules: &IgnoreRules,
    excluded: &PathGlobs,
    relative: &str,
    is_dir: bool,
) -> Option<crate::workspace::ignore_rules::FolderChain> {
    if !relevant(relative) {
        return None;
    }
    let mut prefix = String::new();
    for part in relative.split('/') {
        if !prefix.is_empty() {
            prefix.push('/');
        }
        prefix.push_str(part);
        if excluded.matches(&prefix) {
            return None;
        }
    }
    let parent = relative.rsplit_once('/').map_or("", |(parent, _)| parent);
    let (parent_chain, ignored) = rules.chain_for(parent, RuleFiles::probe(&root.join(parent)));
    if ignored || rules.is_ignored(&parent_chain, relative, is_dir) {
        return None;
    }
    // A folder's own rules are read when the walk lists it.
    Some(parent_chain)
}
