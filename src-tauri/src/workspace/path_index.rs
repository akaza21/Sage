//! Quick Open's in-memory path index (EDIT-0005 AD-4). It holds the active
//! Project's File paths, gathered with the same rules Search uses.
//!
//! The first query builds it. After that, each new Quick Open session walks
//! the Project again in the background while queries keep answering from the
//! current index, as VS Code lists the workspace afresh for each Quick Open
//! session. File events aren't used: on macOS and Linux the watcher covers at
//! most 64 folders and doesn't say which folder an event is in, so it can't
//! keep a list of every File current. One index exists at a time, and it is
//! dropped when its Project closes or is switched away from.

mod arena;
mod build;
pub(crate) mod command;
mod query;
mod score;
#[cfg(test)]
mod tests;

use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex, MutexGuard, TryLockError,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use crate::workspace::{
    file_walk::MAX_WORKSPACE_FILES, ignore_rules::IgnoreRules, path_glob::PathGlobs,
};

use arena::PathArena;
use build::{Walk, build_threads};

/// At most this many Files are indexed across a Project's roots.
const MAX_INDEXED_FILES: usize = MAX_WORKSPACE_FILES as usize;

/// What an index was built from. A query with different inputs rebuilds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct IndexInputs {
    pub(crate) project_id: String,
    /// Root IDs with their canonical paths, in Project order.
    pub(crate) roots: Vec<(String, PathBuf)>,
    pub(crate) use_ignore_files: bool,
    pub(crate) files_exclude: Vec<String>,
    pub(crate) global_excludes: Option<PathBuf>,
}

struct IndexedRoot {
    root_id: String,
    arena: PathArena,
    truncated: bool,
}

struct ProjectIndex {
    inputs: IndexInputs,
    roots: Vec<IndexedRoot>,
    /// The Quick Open session it was built for.
    session: u64,
}

impl ProjectIndex {
    fn files(&self) -> usize {
        self.roots.iter().map(|root| root.arena.len()).sum()
    }

    fn heap_bytes(&self) -> usize {
        self.roots.iter().map(|root| root.arena.heap_bytes()).sum()
    }

    fn truncated(&self) -> bool {
        self.roots.iter().any(|root| root.truncated)
    }
}

struct Slot {
    /// Queries rank a shared handle, so the lock is held only to swap it.
    index: Option<Arc<ProjectIndex>>,
    /// A background walk is building the next index.
    refreshing: bool,
}

static INDEX: Mutex<Slot> = Mutex::new(Slot {
    index: None,
    refreshing: false,
});
static LATEST_GENERATION: AtomicU64 = AtomicU64::new(0);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poison| poison.into_inner())
}

/// Drops the index of a Project that is closing, freeing its memory. It
/// never waits: it runs on the command thread, so while a first build holds
/// the lock, the drop is left to a short-lived thread.
pub(crate) fn forget_project(project_id: &str) {
    fn forget(slot: &mut Slot, project_id: &str) {
        if slot
            .index
            .as_ref()
            .is_some_and(|index| index.inputs.project_id == project_id)
        {
            slot.index = None;
        }
    }
    match INDEX.try_lock() {
        Ok(mut slot) => forget(&mut slot, project_id),
        Err(TryLockError::Poisoned(poison)) => forget(&mut poison.into_inner(), project_id),
        Err(TryLockError::WouldBlock) => {
            let project_id = project_id.to_owned();
            let _ = std::thread::Builder::new()
                .name("quick-open-forget".into())
                .spawn(move || forget(&mut lock(&INDEX), &project_id));
        }
    }
}

/// Records `generation` as the newest query and says whether it still is.
fn claim_generation(generation: u64) -> bool {
    LATEST_GENERATION.fetch_max(generation, Ordering::AcqRel) <= generation
}

fn superseded(generation: u64) -> bool {
    LATEST_GENERATION.load(Ordering::Acquire) > generation
}

/// How the index was readied for a query.
pub(crate) struct Readied {
    /// This query walked the Project first.
    pub(crate) built: bool,
    /// A newer index is being built; asking again shortly sees it.
    pub(crate) refreshing: bool,
    pub(crate) took: Duration,
}

/// The index to answer `inputs` from. Without one for these inputs, it is
/// built now, under the lock, so a burst of first queries builds it once.
/// With one from an earlier Quick Open session, it answers at once and a
/// fresh one is walked in the background.
fn ready(inputs: &IndexInputs, session: u64) -> (Arc<ProjectIndex>, Readied) {
    let started = Instant::now();
    let mut slot = lock(&INDEX);
    if let Some(index) = slot.index.clone().filter(|index| &index.inputs == inputs) {
        if index.session != session && !slot.refreshing {
            slot.refreshing = refresh_in_background(Arc::clone(&index), session);
        }
        let refreshing = slot.refreshing;
        return (
            index,
            Readied {
                built: false,
                refreshing,
                took: started.elapsed(),
            },
        );
    }
    // Free the old index before building the new one.
    slot.index = None;
    let index = Arc::new(build_index(inputs, session, None));
    slot.index = Some(Arc::clone(&index));
    (
        index,
        Readied {
            built: true,
            refreshing: false,
            took: started.elapsed(),
        },
    )
}

/// Walks the Project on another thread and swaps the result in, unless the
/// index was dropped or rebuilt for other inputs meanwhile. Returns whether
/// the thread started.
fn refresh_in_background(current: Arc<ProjectIndex>, session: u64) -> bool {
    std::thread::Builder::new()
        .name("quick-open-refresh".into())
        .spawn(move || {
            let inputs = current.inputs.clone();
            let fresh = build_index(&inputs, session, Some(&current));
            drop(current);
            let mut slot = lock(&INDEX);
            slot.refreshing = false;
            if slot
                .index
                .as_ref()
                .is_some_and(|index| index.inputs == inputs)
            {
                slot.index = Some(Arc::new(fresh));
            }
        })
        .is_ok()
}

fn build_index(
    inputs: &IndexInputs,
    session: u64,
    previous: Option<&ProjectIndex>,
) -> ProjectIndex {
    build_index_within(inputs, session, previous, MAX_INDEXED_FILES)
}

/// Walks every root. Each may fill an even share of the `limit` Files still
/// allowed, so one large root can't leave the others out. A root's arena is
/// sized from what `previous` held for it.
fn build_index_within(
    inputs: &IndexInputs,
    session: u64,
    previous: Option<&ProjectIndex>,
    limit: usize,
) -> ProjectIndex {
    let excluded = PathGlobs::new(&inputs.files_exclude);
    let threads = build_threads();
    let count = inputs.roots.len();
    let mut used = 0;
    let mut roots = Vec::with_capacity(count);
    for (position, (root_id, root)) in inputs.roots.iter().enumerate() {
        let rules = IgnoreRules::load(
            root,
            inputs.use_ignore_files,
            inputs.global_excludes.as_deref(),
        );
        let walked = Walk {
            rules: &rules,
            excluded: &excluded,
            room: (limit - used) / (count - position),
            threads,
            expect: previous
                .and_then(|index| index.roots.iter().find(|old| &old.root_id == root_id))
                .map_or((0, 0), |old| (old.arena.len(), old.arena.text_len())),
        }
        .run(root);
        used += walked.arena.len();
        roots.push(IndexedRoot {
            root_id: root_id.clone(),
            arena: walked.arena,
            truncated: walked.truncated,
        });
    }
    ProjectIndex {
        inputs: inputs.clone(),
        roots,
        session,
    }
}
