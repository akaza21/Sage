//! Walks a root for the path index. It keeps what Search would scan:
//! excluded and ignored folders are not entered, symlinks are skipped, `.git`
//! and `node_modules` are always left out, and folders deeper than Search
//! goes are not entered.
//!
//! The walk runs on one thread. Four threads walked 200k Files in 0.08 s
//! instead of 0.2 s, but glibc gives each thread its own memory pool and
//! keeps what a walk freed there, so the process stayed 50 to 150 MiB larger
//! afterwards; on one thread it returns to where it started.

use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::workspace::{
    directory_skip::skip_workspace_directory_name,
    ignore_rules::{FolderChain, IgnoreRules, RuleFiles},
    path_glob::PathGlobs,
    search::MAX_SEARCH_DEPTH,
};

use super::arena::{MAX_INDEXED_PATH_BYTES, PathArena};

/// A walk stops, marked truncated, after entering this many folders, so a
/// tree of empty folders can't grow the stack without bound.
const MAX_WALK_FOLDERS: usize = 100_000;

/// A folder still to walk.
struct Folder {
    path: PathBuf,
    relative: String,
    chain: FolderChain,
    depth: usize,
}

/// What one walk found.
pub(super) struct Walked {
    pub(super) arena: PathArena,
    pub(super) truncated: bool,
}

pub(super) struct Walk<'a> {
    pub(super) rules: &'a IgnoreRules,
    pub(super) excluded: &'a PathGlobs,
    /// Files this walk may add.
    pub(super) room: usize,
    /// Paths and bytes the root held last time, to size the arena once.
    pub(super) expect: (usize, usize),
}

/// How far a walk has got.
#[derive(Default)]
struct Counts {
    files: usize,
    folders: usize,
    full: bool,
}

impl Walk<'_> {
    /// Walks `root` into a new arena.
    pub(super) fn run(&self, root: &Path) -> Walked {
        let (paths, bytes) = self.expect;
        let mut arena = PathArena::with_capacity(paths + paths / 16, bytes + bytes / 16);
        let mut counts = Counts::default();
        let mut stack = vec![Folder {
            path: root.to_path_buf(),
            relative: String::new(),
            chain: FolderChain::default(),
            depth: 0,
        }];
        while let Some(folder) = stack.pop() {
            self.visit(folder, &mut arena, &mut stack, &mut counts);
            if counts.full {
                break;
            }
        }
        arena.trim_slack();
        Walked {
            arena,
            truncated: counts.full,
        }
    }

    /// Lists one folder: its Files go into `found`, its admitted subfolders
    /// onto `stack`.
    fn visit(
        &self,
        folder: Folder,
        found: &mut PathArena,
        stack: &mut Vec<Folder>,
        counts: &mut Counts,
    ) {
        let Ok(listing) = fs::read_dir(&folder.path) else {
            return;
        };
        let mut entries = Vec::new();
        let mut rule_files = RuleFiles::default();
        for entry in listing.flatten() {
            let name = entry.file_name();
            rule_files.note(&name);
            if !skip_workspace_directory_name(&name) {
                entries.push((entry, name));
            }
        }
        let chain = self
            .rules
            .enter(&folder.chain, &folder.relative, rule_files);
        for (entry, name) in entries {
            let Ok(name) = name.into_string() else {
                continue;
            };
            if name.contains('/') || name.contains('\\') {
                continue;
            }
            let relative = if folder.relative.is_empty() {
                name
            } else {
                format!("{}/{name}", folder.relative)
            };
            if relative.len() > MAX_INDEXED_PATH_BYTES {
                continue;
            }
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink()
                || self.excluded.matches(&relative)
                || self.rules.is_ignored(&chain, &relative, file_type.is_dir())
            {
                continue;
            }
            if file_type.is_dir() {
                if folder.depth >= MAX_SEARCH_DEPTH {
                    continue;
                }
                if counts.folders >= MAX_WALK_FOLDERS {
                    counts.full = true;
                    return;
                }
                counts.folders += 1;
                stack.push(Folder {
                    path: entry.path(),
                    relative,
                    chain: chain.clone(),
                    depth: folder.depth + 1,
                });
            } else if file_type.is_file() {
                if counts.files >= self.room {
                    counts.full = true;
                    return;
                }
                counts.files += 1;
                found.push(&relative);
            }
        }
    }
}
