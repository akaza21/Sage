//! Quick Open's in-memory path index (EDIT-0005 AD-4). It holds the active
//! Project's File paths, gathered with the same rules Search uses.
//!
//! The first query builds it. After that, each new Quick Open session walks
//! the Project again in the background while queries keep answering from the
//! current index, as VS Code lists the workspace afresh for each Quick Open
//! session. File events aren't used: on macOS and Linux the watcher covers at
//! most 64 folders and doesn't say which folder an event is in, so it can't
//! keep a list of every File current. One index exists at a time, and it is
//! dropped when its Project closes or is switched away from. Every walk runs
//! on one long-lived thread (see `on_walker`).

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
        Arc, Mutex, MutexGuard,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use crate::workspace::{
    file_walk::MAX_WORKSPACE_FILES, ignore_rules::IgnoreRules, path_glob::PathGlobs,
};

use arena::PathArena;
use build::Walk;

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
    /// Queries rank a shared handle, so the lock is held only to look it up
    /// or swap it.
    index: Option<Arc<ProjectIndex>>,
    /// A background walk is building the next index.
    refreshing: bool,
    /// Bumped by every forget; a walk started before it keeps nothing.
    epoch: u64,
}

static INDEX: Mutex<Slot> = Mutex::new(Slot {
    index: None,
    refreshing: false,
    epoch: 0,
});
/// Held while a first build walks, so a burst of first queries builds once.
static BUILDING: Mutex<()> = Mutex::new(());
static LATEST_GENERATION: AtomicU64 = AtomicU64::new(0);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poison| poison.into_inner())
}

/// Drops the index of a Project that is closing, freeing its memory. The
/// lock is only ever held briefly, so this doesn't wait on a walk; a walk
/// that ends after it sees the bumped epoch and keeps nothing.
pub(crate) fn forget_project(project_id: &str) {
    let mut slot = lock(&INDEX);
    slot.epoch += 1;
    if slot
        .index
        .as_ref()
        .is_some_and(|index| index.inputs.project_id == project_id)
    {
        slot.index = None;
    }
    drop(slot);
    release_freed_memory();
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
/// built now; queries that arrive meanwhile wait for that build rather than
/// start another. With one from an earlier Quick Open session, it answers
/// at once and a fresh one is walked in the background.
fn ready(inputs: &IndexInputs, session: u64) -> (Arc<ProjectIndex>, Readied) {
    let started = Instant::now();
    let readied = |index, built, refreshing| {
        let took = started.elapsed();
        (
            index,
            Readied {
                built,
                refreshing,
                took,
            },
        )
    };
    if let Some((index, refreshing)) = current(inputs, session) {
        return readied(index, false, refreshing);
    }
    let _building = lock(&BUILDING);
    if let Some((index, refreshing)) = current(inputs, session) {
        return readied(index, false, refreshing);
    }
    let epoch = {
        let mut slot = lock(&INDEX);
        // Free the old index before building the new one.
        slot.index = None;
        slot.epoch
    };
    let index = Arc::new(walk(inputs, session));
    let mut slot = lock(&INDEX);
    if slot.epoch == epoch {
        slot.index = Some(Arc::clone(&index));
    }
    readied(index, true, false)
}

/// The index for `inputs` if there is one, starting a background refresh
/// when it was built for another session, and whether one is running.
fn current(inputs: &IndexInputs, session: u64) -> Option<(Arc<ProjectIndex>, bool)> {
    let mut slot = lock(&INDEX);
    let index = slot.index.clone().filter(|index| &index.inputs == inputs)?;
    if index.session != session && !slot.refreshing {
        slot.refreshing = refresh_in_background(Arc::clone(&index), session, slot.epoch);
    }
    Some((index, slot.refreshing))
}

/// Walks the Project again and swaps the result in, unless the Project was
/// forgotten or rebuilt for other inputs meanwhile. Returns whether the walk
/// was queued.
fn refresh_in_background(current: Arc<ProjectIndex>, session: u64, epoch: u64) -> bool {
    on_walker(Box::new(move || {
        let inputs = current.inputs.clone();
        let fresh = build_index(&inputs, session, Some(&current));
        drop(current);
        let mut slot = lock(&INDEX);
        slot.refreshing = false;
        if slot.epoch == epoch
            && slot
                .index
                .as_ref()
                .is_some_and(|index| index.inputs == inputs)
        {
            slot.index = Some(Arc::new(fresh));
        }
    }))
}

/// Builds an index on the walker thread and waits for it, or builds it here
/// if that thread can't be reached.
fn walk(inputs: &IndexInputs, session: u64) -> ProjectIndex {
    let (done, built) = mpsc::sync_channel(1);
    let queued = {
        let inputs = inputs.clone();
        on_walker(Box::new(move || {
            let _ = done.send(build_index(&inputs, session, None));
        }))
    };
    queued
        .then(|| built.recv().ok())
        .flatten()
        .unwrap_or_else(|| build_index(inputs, session, None))
}

type Job = Box<dyn FnOnce() + Send>;

/// Hands memory freed by a walk or a dropped index back to the system. With
/// every walk on one thread, glibc's pools hold one walk's worth of freed
/// memory (about as much again as the index); this returns it. Other
/// allocators return large freed blocks on their own.
fn release_freed_memory() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    // SAFETY: `malloc_trim` takes no pointers; it only returns free pages
    // and may be called from any thread.
    unsafe {
        libc::malloc_trim(0);
    }
}

/// Runs `job` on the one thread that does every walk. glibc gives each
/// thread its own memory pool and keeps what is freed there; with walks
/// spread over many threads the process stayed 50 to 150 MiB larger after
/// them, while one thread reuses the same memory every time. Returns false
/// if the thread couldn't be started.
fn on_walker(job: Job) -> bool {
    static WALKER: Mutex<Option<mpsc::Sender<Job>>> = Mutex::new(None);
    let mut walker = lock(&WALKER);
    let job = match walker.as_ref() {
        Some(sender) => match sender.send(job) {
            Ok(()) => return true,
            // The thread is gone; start another.
            Err(mpsc::SendError(job)) => job,
        },
        None => job,
    };
    let (sender, jobs) = mpsc::channel::<Job>();
    let started = std::thread::Builder::new()
        .name("quick-open-walk".into())
        .spawn(move || {
            for job in jobs {
                job();
                release_freed_memory();
            }
        });
    if started.is_err() || sender.send(job).is_err() {
        return false;
    }
    *walker = Some(sender);
    true
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
