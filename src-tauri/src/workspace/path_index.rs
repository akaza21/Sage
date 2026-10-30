//! Quick Open's in-memory path index (EDIT-0005 AD-4). It holds the active
//! project's File paths, built on the first query from the same rules Search
//! uses, then kept current from file events. One index exists at a time and
//! it is dropped when its project stops being watched. An index built while
//! a root had no watcher can't be kept current, so it is rebuilt each query
//! until every root is watched.

mod arena;
mod build;
pub(crate) mod command;
mod query;
mod refresh;
mod score;
#[cfg(test)]
mod tests;

use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::{
        Mutex, MutexGuard,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use crate::workspace::{
    file_walk::MAX_WORKSPACE_FILES, ignore_rules::IgnoreRules, notify::WorkspaceNotifyNotice,
    path_glob::PathGlobs,
};

use arena::PathArena;
use build::{Walk, build_root, build_threads};
use refresh::{MAX_REFRESH_PATHS, Refresh};

/// At most this many Files are indexed across a project's roots.
const MAX_INDEXED_FILES: usize = MAX_WORKSPACE_FILES as usize;

/// What an index was built from. A query with different inputs rebuilds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct IndexInputs {
    pub(crate) project_id: String,
    /// Root IDs with their canonical paths, in project order.
    pub(crate) roots: Vec<(String, PathBuf)>,
    pub(crate) use_ignore_files: bool,
    pub(crate) files_exclude: Vec<String>,
    pub(crate) global_excludes: Option<PathBuf>,
}

struct IndexedRoot {
    root_id: String,
    root: PathBuf,
    arena: PathArena,
    rules: IgnoreRules,
    truncated: bool,
}

struct ProjectIndex {
    inputs: IndexInputs,
    excluded: PathGlobs,
    roots: Vec<IndexedRoot>,
    /// Every root had a watcher when it was built, so events keep it current.
    watched: bool,
}

impl ProjectIndex {
    fn files(&self) -> usize {
        self.roots.iter().map(|root| root.arena.live()).sum()
    }

    fn heap_bytes(&self) -> usize {
        self.roots.iter().map(|root| root.arena.heap_bytes()).sum()
    }

    fn truncated(&self) -> bool {
        self.roots.iter().any(|root| root.truncated)
    }
}

/// Changed paths reported since the last query, per root.
#[derive(Default)]
struct Pending {
    project_id: Option<String>,
    paths: BTreeMap<String, BTreeSet<String>>,
    /// Roots that lost events or changed too much: walk them again.
    rebuild: BTreeSet<String>,
}

static INDEX: Mutex<Option<ProjectIndex>> = Mutex::new(None);
static PENDING: Mutex<Option<Pending>> = Mutex::new(None);
static LATEST_GENERATION: AtomicU64 = AtomicU64::new(0);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poison| poison.into_inner())
}

/// Notes a file event for the index. Cheap: the path is looked at when Quick
/// Open next queries.
pub(crate) fn note_change(notice: &WorkspaceNotifyNotice) {
    if notice.kind == "changed"
        || (notice.kind != "resync" && !refresh::relevant(&notice.relative_path))
    {
        return;
    }
    let mut guard = lock(&PENDING);
    let pending = guard.get_or_insert_with(Pending::default);
    if pending.project_id.as_deref() != Some(&notice.project_id) {
        *pending = Pending {
            project_id: Some(notice.project_id.clone()),
            ..Pending::default()
        };
    }
    if notice.kind == "resync" || pending.rebuild.contains(&notice.root_id) {
        pending.paths.remove(&notice.root_id);
        pending.rebuild.insert(notice.root_id.clone());
        return;
    }
    let paths = pending.paths.entry(notice.root_id.clone()).or_default();
    paths.insert(notice.relative_path.clone());
    if paths.len() > MAX_REFRESH_PATHS {
        pending.paths.remove(&notice.root_id);
        pending.rebuild.insert(notice.root_id.clone());
    }
}

/// Drops the index of a project that is no longer watched, freeing its memory.
pub(crate) fn forget_project(project_id: &str) {
    let mut index = lock(&INDEX);
    if index
        .as_ref()
        .is_some_and(|index| index.inputs.project_id == project_id)
    {
        *index = None;
    }
    drop(index);
    let mut pending = lock(&PENDING);
    if pending
        .as_ref()
        .is_some_and(|pending| pending.project_id.as_deref() == Some(project_id))
    {
        *pending = None;
    }
}

/// Whether the project's file watcher covers every root in `inputs`.
fn watched(inputs: &IndexInputs) -> bool {
    crate::workspace::notify::workspace_notify_bound_roots(&inputs.project_id).is_some_and(
        |watched| {
            inputs
                .roots
                .iter()
                .all(|(root_id, _)| watched.contains(root_id))
        },
    )
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
    pub(crate) built: bool,
    pub(crate) took: Duration,
}

/// Brings the index up to date for `inputs`: builds it when there is none,
/// its inputs changed or it wasn't watched, and otherwise applies the changes
/// noted since. `watched` says whether every root has a watcher now; it must
/// be read while the index is locked, so an unwatch that forgets the index
/// can't slip between the read and the build.
fn ready(slot: &mut Option<ProjectIndex>, inputs: &IndexInputs, watched: bool) -> Readied {
    let started = Instant::now();
    let stale = slot
        .as_ref()
        .is_none_or(|index| &index.inputs != inputs || !index.watched);
    if stale {
        // Free the old index before building the new one.
        *slot = None;
        take_pending(&inputs.project_id);
        *slot = Some(build_index(inputs, watched));
        return Readied {
            built: true,
            took: started.elapsed(),
        };
    }
    let index = slot.as_mut().expect("index checked above");
    let Some(pending) = take_pending(&inputs.project_id) else {
        return Readied {
            built: false,
            took: started.elapsed(),
        };
    };
    let mut built = false;
    for position in 0..index.roots.len() {
        let root_id = index.roots[position].root_id.clone();
        let changed = pending
            .paths
            .get(&root_id)
            .map(|paths| paths.iter().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        let mut rebuild = pending.rebuild.contains(&root_id);
        if !rebuild && !changed.is_empty() {
            let others = index.files() - index.roots[position].arena.live();
            let ProjectIndex {
                excluded, roots, ..
            } = index;
            let root = &mut roots[position];
            let walk = Walk {
                rules: &root.rules,
                excluded,
                room: MAX_INDEXED_FILES.saturating_sub(others + root.arena.live()),
                threads: 1,
            };
            rebuild =
                refresh::apply(&root.root, &mut root.arena, &changed, &walk) == Refresh::Rebuild;
        }
        if rebuild {
            rebuild_root(index, position);
            built = true;
        }
    }
    Readied {
        built,
        took: started.elapsed(),
    }
}

fn take_pending(project_id: &str) -> Option<Pending> {
    let mut guard = lock(&PENDING);
    let pending = guard.take()?;
    (pending.project_id.as_deref() == Some(project_id)).then_some(pending)
}

fn build_index(inputs: &IndexInputs, watched: bool) -> ProjectIndex {
    let mut index = ProjectIndex {
        watched,
        inputs: inputs.clone(),
        excluded: PathGlobs::new(&inputs.files_exclude),
        roots: inputs
            .roots
            .iter()
            .map(|(root_id, root)| IndexedRoot {
                root_id: root_id.clone(),
                root: root.clone(),
                arena: PathArena::default(),
                rules: IgnoreRules::load(
                    root,
                    inputs.use_ignore_files,
                    inputs.global_excludes.as_deref(),
                ),
                truncated: false,
            })
            .collect(),
    };
    for position in 0..index.roots.len() {
        rebuild_root(&mut index, position);
    }
    index
}

fn rebuild_root(index: &mut ProjectIndex, position: usize) {
    index.roots[position].arena = PathArena::default();
    let room = MAX_INDEXED_FILES.saturating_sub(index.files());
    let ProjectIndex {
        excluded, roots, ..
    } = index;
    let root = &mut roots[position];
    let walked = build_root(
        &root.root,
        &Walk {
            rules: &root.rules,
            excluded,
            room,
            threads: build_threads(),
        },
    );
    root.arena = walked.arena;
    root.truncated = walked.truncated;
}
