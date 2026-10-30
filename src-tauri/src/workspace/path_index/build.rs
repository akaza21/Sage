//! Walks a root for the path index on up to four threads. It keeps what
//! Search would scan: excluded and ignored folders are not entered,
//! symlinks are skipped, `.git` and `node_modules` are always left out, and
//! folders deeper than Search goes are not entered.

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
    search::MAX_SEARCH_DEPTH,
};

use super::arena::{MAX_INDEXED_PATH_BYTES, PathArena};

/// A walk stops, marked truncated, after entering this many folders, so a
/// tree of empty folders can't grow the queue without bound.
const MAX_WALK_FOLDERS: usize = 100_000;
/// Each worker hands its paths over in batches of this many.
const BATCH_PATHS: usize = 256;

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
    pub(super) threads: usize,
    /// Paths and bytes the root held last time, to size the arena once.
    pub(super) expect: (usize, usize),
}

/// Threads for a build or a ranking: at most four, and at most the
/// machine's cores.
pub(super) fn build_threads() -> usize {
    std::thread::available_parallelism().map_or(1, |cores| cores.get().min(4))
}

struct Queue {
    folders: Vec<Folder>,
    busy: usize,
}

/// Counts shared by a walk's workers.
#[derive(Default)]
struct Counts {
    files: AtomicUsize,
    folders: AtomicUsize,
    full: AtomicBool,
}

impl Walk<'_> {
    /// Walks `root` into a new arena. Workers hand paths over in small
    /// batches straight into the one arena that is kept, so a walk allocates
    /// little besides it. (Large short-lived buffers would stay in glibc's
    /// per-thread pools after the walk, keeping the process tens of MiB
    /// larger than the index.)
    pub(super) fn run(&self, root: &Path) -> Walked {
        let queue = Mutex::new(Queue {
            folders: vec![Folder {
                path: root.to_path_buf(),
                relative: String::new(),
                chain: FolderChain::default(),
                depth: 0,
            }],
            busy: 0,
        });
        let wake = Condvar::new();
        let counts = Counts::default();
        let (paths, bytes) = self.expect;
        let kept = Mutex::new(PathArena::with_capacity(
            paths + paths / 16,
            bytes + bytes / 16,
        ));
        std::thread::scope(|scope| {
            for _ in 0..self.threads.max(1) {
                scope.spawn(|| self.worker(&queue, &wake, &counts, &kept));
            }
        });
        let mut arena = kept
            .into_inner()
            .unwrap_or_else(|poison| poison.into_inner());
        arena.trim_slack();
        Walked {
            arena,
            truncated: counts.full.load(Ordering::Relaxed),
        }
    }

    fn worker(
        &self,
        queue: &Mutex<Queue>,
        wake: &Condvar,
        counts: &Counts,
        kept: &Mutex<PathArena>,
    ) {
        let mut found = PathArena::default();
        let hand_over = |found: &mut PathArena| {
            kept.lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .take_from(found);
        };
        loop {
            let folder = {
                let mut state = queue.lock().unwrap_or_else(|poison| poison.into_inner());
                loop {
                    if counts.full.load(Ordering::Relaxed) {
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
                hand_over(&mut found);
                return;
            };
            let children = self.visit(folder, &mut found, counts);
            if found.len() >= BATCH_PATHS {
                hand_over(&mut found);
            }
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

    /// Lists one folder: its Files go into `found`, its admitted subfolders
    /// are returned.
    fn visit(&self, folder: Folder, found: &mut PathArena, counts: &Counts) -> Vec<Folder> {
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
                if folder.depth >= MAX_SEARCH_DEPTH {
                    continue;
                }
                if counts.folders.fetch_add(1, Ordering::Relaxed) >= MAX_WALK_FOLDERS {
                    counts.full.store(true, Ordering::Relaxed);
                    return Vec::new();
                }
                children.push(Folder {
                    path: entry.path(),
                    relative,
                    chain: chain.clone(),
                    depth: folder.depth + 1,
                });
            } else if file_type.is_file() {
                if counts.files.fetch_add(1, Ordering::Relaxed) >= self.room {
                    counts.full.store(true, Ordering::Relaxed);
                    return Vec::new();
                }
                found.push(&relative);
            }
        }
        children
    }
}
