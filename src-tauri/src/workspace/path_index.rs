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
        atomic::{AtomicBool, AtomicU64, Ordering},
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
    /// Counts forgets.
    epoch: u64,
    /// The epoch at which each Project closed in this run was last
    /// forgotten, so a query checked before that keeps nothing.
    forgotten: Vec<(String, u64)>,
    /// Walks under way, by Project, each with the flag that stops it.
    walks: Vec<(String, Arc<AtomicBool>)>,
}

impl Slot {
    fn forgotten_since(&self, project_id: &str, ticket: Ticket) -> bool {
        self.forgotten
            .iter()
            .any(|(id, epoch)| id == project_id && *epoch > ticket.0)
    }

    /// Stops every walk, or only those of one Project.
    fn stop_walks(&self, project_id: Option<&str>) {
        for (id, stop) in &self.walks {
            if project_id.is_none_or(|project_id| project_id == id) {
                stop.store(true, Ordering::Release);
            }
        }
    }

    /// Registers a walk of `project_id`; the returned guard ends it.
    fn begin(&mut self, project_id: &str, refresh: bool) -> Walking {
        let stop = Arc::new(AtomicBool::new(false));
        self.walks.push((project_id.to_owned(), Arc::clone(&stop)));
        Walking { stop, refresh }
    }
}

/// Ends a walk when dropped, even if it panicked or never ran: it is
/// unregistered and, for a refresh, `refreshing` is cleared. Never dropped
/// while `INDEX` is locked.
struct Walking {
    stop: Arc<AtomicBool>,
    refresh: bool,
}

impl Walking {
    fn stopped(&self) -> bool {
        self.stop.load(Ordering::Acquire)
    }
}

impl Drop for Walking {
    fn drop(&mut self) {
        let mut slot = lock(&INDEX);
        slot.walks
            .retain(|(_, stop)| !Arc::ptr_eq(stop, &self.stop));
        if self.refresh {
            slot.refreshing = false;
        }
    }
}

static INDEX: Mutex<Slot> = Mutex::new(Slot {
    index: None,
    refreshing: false,
    epoch: 0,
    forgotten: Vec::new(),
    walks: Vec::new(),
});
/// Held while a first build walks, so a burst of first queries builds once.
static BUILDING: Mutex<()> = Mutex::new(());
static LATEST_GENERATION: AtomicU64 = AtomicU64::new(0);

/// How long a first build waits for the walker thread to take it up before
/// walking on the calling thread instead. The walker only falls this far
/// behind if a folder read hangs, as on an unreachable network drive.
const WALKER_WAIT: Duration = Duration::from_secs(2);
/// Memory is handed back after walks and drops of indexes at least this
/// large (about 13,000 paths); below it there is little to return.
const TRIM_FROM: usize = 1 << 20;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poison| poison.into_inner())
}

/// When a query was checked against the active Project. Taken before that
/// check; if the Project is forgotten after it, the query keeps nothing.
#[derive(Clone, Copy)]
pub(crate) struct Ticket(u64);

pub(crate) fn ticket() -> Ticket {
    Ticket(lock(&INDEX).epoch)
}

/// Drops the index of a Project that is closing and stops its walks. The
/// lock is only ever held briefly, so this doesn't wait on a walk, and the
/// index is freed on the walker thread.
pub(crate) fn forget_project(project_id: &str) {
    let mut slot = lock(&INDEX);
    slot.epoch += 1;
    let epoch = slot.epoch;
    match slot.forgotten.iter_mut().find(|(id, _)| id == project_id) {
        Some(entry) => entry.1 = epoch,
        None => slot.forgotten.push((project_id.to_owned(), epoch)),
    }
    slot.stop_walks(Some(project_id));
    let dropped = slot
        .index
        .take_if(|index| index.inputs.project_id == project_id);
    drop(slot);
    if let Some(index) = dropped {
        free_on_walker(index);
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
/// built now; queries that arrive meanwhile wait for that build rather than
/// start another. With one from an earlier Quick Open session, it answers
/// at once and a fresh one is walked in the background. `None` when the
/// Project was forgotten after `ticket`.
fn ready(
    inputs: &IndexInputs,
    session: u64,
    ticket: Ticket,
) -> Option<(Arc<ProjectIndex>, Readied)> {
    let started = Instant::now();
    let readied = |index, built, refreshing| {
        let took = started.elapsed();
        Some((
            index,
            Readied {
                built,
                refreshing,
                took,
            },
        ))
    };
    if let Some((index, refreshing)) = current(inputs, session) {
        return readied(index, false, refreshing);
    }
    let _building = lock(&BUILDING);
    if let Some((index, refreshing)) = current(inputs, session) {
        return readied(index, false, refreshing);
    }
    let walking = {
        let mut slot = lock(&INDEX);
        if slot.forgotten_since(&inputs.project_id, ticket) {
            return None;
        }
        // Free the old index before building the new one, and stop any
        // refresh of it so this walk isn't queued behind one.
        if let Some(old) = slot.index.take() {
            free_on_walker(old);
        }
        slot.stop_walks(None);
        slot.begin(&inputs.project_id, false)
    };
    let index = Arc::new(walk(inputs, session, &walking.stop));
    let stopped = walking.stopped();
    drop(walking);
    let mut slot = lock(&INDEX);
    if stopped || slot.forgotten_since(&inputs.project_id, ticket) {
        return None;
    }
    slot.index = Some(Arc::clone(&index));
    drop(slot);
    readied(index, true, false)
}

/// The index for `inputs` if there is one, starting a background refresh
/// when it was built for another session, and whether one is running.
fn current(inputs: &IndexInputs, session: u64) -> Option<(Arc<ProjectIndex>, bool)> {
    let mut slot = lock(&INDEX);
    let index = slot.index.clone().filter(|index| &index.inputs == inputs)?;
    if index.session == session || slot.refreshing {
        return Some((index, slot.refreshing));
    }
    slot.refreshing = true;
    let walking = slot.begin(&inputs.project_id, true);
    drop(slot);
    // Queued without the lock held: a job that can't be queued is dropped
    // here, and dropping `walking` takes the lock.
    let queued = refresh_in_background(Arc::clone(&index), session, walking);
    Some((index, queued))
}

/// Walks the Project again and swaps the result in, unless the walk was
/// stopped or the index was rebuilt for other inputs meanwhile. Returns
/// whether the walk was queued.
fn refresh_in_background(current: Arc<ProjectIndex>, session: u64, walking: Walking) -> bool {
    on_walker(Box::new(move || {
        let inputs = current.inputs.clone();
        let fresh = build_index(&inputs, session, Some(&current), &walking.stop);
        drop(current);
        let large = fresh.heap_bytes() >= TRIM_FROM;
        let mut slot = lock(&INDEX);
        let replaced = if !walking.stopped()
            && slot
                .index
                .as_ref()
                .is_some_and(|index| index.inputs == inputs)
        {
            slot.index.replace(Arc::new(fresh))
        } else {
            None
        };
        drop(slot);
        drop(walking);
        drop(replaced);
        large
    }))
}

/// Builds an index on the walker thread and waits for it. It is built here
/// instead if that thread can't be reached or doesn't take it up within
/// `WALKER_WAIT`.
fn walk(inputs: &IndexInputs, session: u64, stop: &Arc<AtomicBool>) -> ProjectIndex {
    let (done, built) = mpsc::sync_channel(1);
    let taken = Arc::new(AtomicBool::new(false));
    let queued = {
        let (inputs, taken, stop) = (inputs.clone(), Arc::clone(&taken), Arc::clone(stop));
        on_walker(Box::new(move || {
            if taken.swap(true, Ordering::AcqRel) {
                return false;
            }
            let index = build_index(&inputs, session, None, &stop);
            let large = index.heap_bytes() >= TRIM_FROM;
            let _ = done.send(index);
            large
        }))
    };
    if queued {
        match built.recv_timeout(WALKER_WAIT) {
            Ok(index) => return index,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // Started already: wait for it. Not yet: it will see
                // `taken` and do nothing.
                if taken.swap(true, Ordering::AcqRel)
                    && let Ok(index) = built.recv()
                {
                    return index;
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {}
        }
    }
    build_index(inputs, session, None, stop)
}

/// Frees `index` on the walker thread, which then hands the memory back.
fn free_on_walker(index: Arc<ProjectIndex>) {
    on_walker(Box::new(move || {
        let large = index.heap_bytes() >= TRIM_FROM;
        drop(index);
        large
    }));
}

/// A walk or a drop, saying whether it freed enough to hand back.
type Job = Box<dyn FnOnce() -> bool + Send>;

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
/// if the thread couldn't be started, having dropped `job`.
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
                if job() {
                    release_freed_memory();
                }
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
    stop: &AtomicBool,
) -> ProjectIndex {
    build_index_within(inputs, session, previous, MAX_INDEXED_FILES, stop)
}

/// Walks every root. Each may fill an even share of the `limit` Files still
/// allowed, so one large root can't leave the others out. A root's arena is
/// sized from what `previous` held for it. A raised `stop` ends the walk
/// early; its result is then discarded.
fn build_index_within(
    inputs: &IndexInputs,
    session: u64,
    previous: Option<&ProjectIndex>,
    limit: usize,
    stop: &AtomicBool,
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
            stop,
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
