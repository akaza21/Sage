//! Walks a root for the path index on up to four threads. It keeps what
//! Search would scan: excluded and ignored folders are not entered,
//! symlinks are skipped, and `.git` and `node_modules` are always left out.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Condvar, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

use crate::workspace::{
    directory_skip::skip_workspace_directory_name,
    ignore_rules::{FolderChain, IgnoreRules, RuleFiles},
    path_glob::PathGlobs,
};

use super::arena::{MAX_INDEXED_PATH_BYTES, PathArena};

const MAX_DEPTH: usize = 64;

/// A folder still to walk.
pub(super) struct Folder {
    pub(super) path: PathBuf,
    pub(super) relative: String,
    pub(super) chain: FolderChain,
    pub(super) depth: usize,
}

/// What one walk added.
pub(super) struct Walked {
    pub(super) arena: PathArena,
    pub(super) truncated: bool,
}

pub(super) struct Walk<'a> {
    pub(super) rules: &'a IgnoreRules,
    pub(super) excluded: &'a PathGlobs,
    /// Files this walk may still add.
    pub(super) room: usize,
    pub(super) threads: usize,
}

/// Threads for a full build: at most four, and at most the machine's cores.
pub(super) fn build_threads() -> usize {
    std::thread::available_parallelism().map_or(1, |cores| cores.get().min(4))
}

/// The root folder of `root`, to start a full walk from.
pub(super) fn root_folder(root: &Path) -> Folder {
    Folder {
        path: root.to_path_buf(),
        relative: String::new(),
        chain: FolderChain::default(),
        depth: 0,
    }
}

struct Queue {
    folders: Vec<Folder>,
    busy: usize,
}

impl Walk<'_> {
    /// Walks every folder in `start` and below, adding the Files it finds
    /// into `into`.
    pub(super) fn run(&self, start: Vec<Folder>, into: &mut PathArena) -> bool {
        let queue = Mutex::new(Queue {
            folders: start,
            busy: 0,
        });
        let wake = Condvar::new();
        let added = AtomicUsize::new(0);
        let full = AtomicBool::new(false);
        let found = std::thread::scope(|scope| {
            let workers = (0..self.threads.max(1))
                .map(|_| scope.spawn(|| self.worker(&queue, &wake, &added, &full)))
                .collect::<Vec<_>>();
            workers
                .into_iter()
                .map(|worker| worker.join().unwrap_or_default())
                .collect::<Vec<_>>()
        });
        let paths = found.iter().map(Vec::len).sum();
        let bytes = found.iter().flatten().map(String::len).sum();
        into.reserve(paths, bytes);
        for path in found.iter().flatten() {
            into.push(path);
        }
        full.load(Ordering::Relaxed)
    }

    fn worker(
        &self,
        queue: &Mutex<Queue>,
        wake: &Condvar,
        added: &AtomicUsize,
        full: &AtomicBool,
    ) -> Vec<String> {
        let mut files = Vec::new();
        loop {
            let folder = {
                let mut state = queue.lock().unwrap_or_else(|poison| poison.into_inner());
                loop {
                    if full.load(Ordering::Relaxed) {
                        break None;
                    }
                    if let Some(folder) = state.folders.pop() {
                        state.busy += 1;
                        break Some(folder);
                    }
                    if state.busy == 0 {
                        break None;
                    }
                    state = wake
                        .wait(state)
                        .unwrap_or_else(|poison| poison.into_inner());
                }
            };
            let Some(folder) = folder else {
                wake.notify_all();
                return files;
            };
            let children = self.visit(folder, &mut files, added, full);
            let mut state = queue.lock().unwrap_or_else(|poison| poison.into_inner());
            state.busy -= 1;
            let woke = !children.is_empty() || state.busy == 0;
            state.folders.extend(children);
            drop(state);
            if woke {
                wake.notify_all();
            }
        }
    }

    /// Lists one folder: its Files go into `files`, its admitted subfolders
    /// are returned.
    fn visit(
        &self,
        folder: Folder,
        files: &mut Vec<String>,
        added: &AtomicUsize,
        full: &AtomicBool,
    ) -> Vec<Folder> {
        let Ok(listing) = fs::read_dir(&folder.path) else {
            return Vec::new();
        };
        let mut entries = Vec::new();
        let mut rule_files = RuleFiles::default();
        for entry in listing.flatten() {
            rule_files.note(&entry.file_name());
            if !skip_workspace_directory_name(&entry.file_name()) {
                entries.push(entry);
            }
        }
        let chain = self
            .rules
            .enter(&folder.chain, &folder.relative, rule_files);
        let mut children = Vec::new();
        for entry in entries {
            let Ok(name) = entry.file_name().into_string() else {
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
                if folder.depth < MAX_DEPTH {
                    children.push(Folder {
                        path: entry.path(),
                        relative,
                        chain: chain.clone(),
                        depth: folder.depth + 1,
                    });
                }
            } else if file_type.is_file() {
                if added.fetch_add(1, Ordering::Relaxed) >= self.room {
                    full.store(true, Ordering::Relaxed);
                    return Vec::new();
                }
                files.push(relative);
            }
        }
        children
    }
}

/// Walks a whole root into a new arena.
pub(super) fn build_root(root: &Path, walk: &Walk<'_>) -> Walked {
    let mut arena = PathArena::default();
    let truncated = walk.run(vec![root_folder(root)], &mut arena);
    Walked { arena, truncated }
}
