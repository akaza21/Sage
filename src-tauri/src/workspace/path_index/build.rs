//! The walk runs on one thread. Four threads walked 200k Files in 0.08 s
//! instead of 0.2 s, but glibc gives each thread its own memory pool and
//! keeps what a walk freed there, so the process stayed 50 to 150 MiB larger
//! afterwards; on one thread it returns to where it started.

use std::{
    collections::VecDeque,
    ffi::OsString,
    fs::{self, DirEntry},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
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

struct Folder {
    path: PathBuf,
    relative: String,
    chain: FolderChain,
    depth: usize,
}

pub(super) struct Walked {
    pub(super) arena: PathArena,
    pub(super) truncated: bool,
}

pub(super) struct Walk<'a> {
    pub(super) rules: &'a IgnoreRules,
    pub(super) excluded: &'a PathGlobs,
    pub(super) stop: &'a AtomicBool,
    pub(super) room: usize,
    pub(super) expect: (usize, usize),
}

#[derive(Default)]
struct Progress {
    files: usize,
    folders: usize,
    full: bool,
    queue: VecDeque<Folder>,
    entries: Vec<(DirEntry, OsString)>,
    relative: String,
}

impl Walk<'_> {
    pub(super) fn run(&self, root: &Path) -> Walked {
        let (paths, bytes) = self.expect;
        let mut arena = PathArena::with_capacity(paths + paths / 16, bytes + bytes / 16);
        let mut progress = Progress {
            queue: VecDeque::from([Folder {
                path: root.to_path_buf(),
                relative: String::new(),
                chain: FolderChain::default(),
                depth: 0,
            }]),
            ..Progress::default()
        };
        while let Some(folder) = progress.queue.pop_front() {
            if self.stop.load(Ordering::Relaxed) {
                break;
            }
            self.visit(folder, &mut arena, &mut progress);
            if progress.full {
                break;
            }
        }
        arena.trim_slack();
        Walked {
            arena,
            truncated: progress.full,
        }
    }

    fn visit(&self, folder: Folder, found: &mut PathArena, progress: &mut Progress) {
        let Ok(listing) = fs::read_dir(&folder.path) else {
            return;
        };
        let mut entries = std::mem::take(&mut progress.entries);
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
        for (entry, name) in entries.drain(..) {
            if !self.admit(&folder, &chain, &entry, &name, found, progress) {
                break;
            }
        }
        progress.entries = entries;
    }

    fn admit(
        &self,
        folder: &Folder,
        chain: &FolderChain,
        entry: &DirEntry,
        name: &OsString,
        found: &mut PathArena,
        progress: &mut Progress,
    ) -> bool {
        let Some(name) = name.to_str() else {
            return true;
        };
        if name.contains('/') || name.contains('\\') {
            return true;
        }
        let relative = &mut progress.relative;
        relative.clear();
        if !folder.relative.is_empty() {
            relative.push_str(&folder.relative);
            relative.push('/');
        }
        relative.push_str(name);
        if relative.len() > MAX_INDEXED_PATH_BYTES {
            return true;
        }
        let Ok(file_type) = entry.file_type() else {
            return true;
        };
        if file_type.is_symlink()
            || self.excluded.matches(relative)
            || self.rules.is_ignored(chain, relative, file_type.is_dir())
        {
            return true;
        }
        if file_type.is_dir() {
            if folder.depth >= MAX_SEARCH_DEPTH {
                return true;
            }
            if progress.folders >= MAX_WALK_FOLDERS {
                progress.full = true;
                return false;
            }
            progress.folders += 1;
            progress.queue.push_back(Folder {
                path: entry.path(),
                relative: relative.clone(),
                chain: chain.clone(),
                depth: folder.depth + 1,
            });
        } else if file_type.is_file() {
            if progress.files >= self.room {
                progress.full = true;
                return false;
            }
            progress.files += 1;
            found.push(relative);
        }
        true
    }
}
