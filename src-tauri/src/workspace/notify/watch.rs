use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use tauri::{AppHandle, Emitter};

use crate::{persistence::Database, state::AppState};

use super::{
    IGNORED_BELOW_NOTIFY_ACTION, OVERFLOW_NOTIFY_ACTION, WORKSPACE_NOTIFY_EVENT,
    WorkspaceNotifyError, WorkspaceNotifyNotice,
    batch::{NoticeSink, spawn_notice_emitter},
    bounded_identifier, registry, reject, relative_path,
    relevance::{NoticeRelevance, Relevance},
};

#[cfg(windows)]
use super::windows;

pub fn relative_from_notify_name(name: &str) -> Result<String, WorkspaceNotifyError> {
    let normalized = name.replace('\\', "/");
    if !relative_path(&normalized) {
        return Err(reject("schema-invalid"));
    }
    Ok(normalized)
}

pub fn authenticate_workspace_notify_root(
    bound_project_id: &str,
    bound_root_id: &str,
    record_project_id: &str,
    absolute_path: &Path,
) -> Result<PathBuf, WorkspaceNotifyError> {
    if !bounded_identifier(bound_project_id) || !bounded_identifier(bound_root_id) {
        return Err(reject("schema-invalid"));
    }
    if bound_project_id != record_project_id || !bounded_identifier(record_project_id) {
        return Err(reject("notify-substituted"));
    }
    let Ok(canonical) = absolute_path.canonicalize() else {
        return Err(reject("notify-substituted"));
    };
    if !canonical.is_dir() {
        return Err(reject("notify-substituted"));
    }
    Ok(canonical)
}

pub fn deliver_workspace_notify_action(
    project_id: &str,
    generation: u32,
    root_id: &str,
    relative: &str,
    action: u32,
) -> Result<Option<WorkspaceNotifyNotice>, WorkspaceNotifyError> {
    let mut registry = registry();
    let Some(watch) = registry.watches.get_mut(project_id) else {
        return Err(reject("notify-substituted"));
    };
    if watch.generation != generation || watch.cancelled.load(Ordering::Acquire) {
        return Ok(None);
    }
    watch.session.classify(root_id, relative, action)
}

/// The delivery of one watched root's records. Each record is weighed by the
/// root's relevance filter, classified against the live watch and queued for
/// the watch's next batch; a record inside an ignored folder is queued as
/// one notice for that folder. Lost records always pass, and make the filter
/// reload.
pub(super) fn root_delivery(
    project_id: String,
    generation: u32,
    root_id: String,
    root: &Path,
    cancelled: Arc<AtomicBool>,
    sink: Arc<NoticeSink>,
) -> impl FnMut(String, u32) + Send + 'static {
    let mut relevance = None;
    let root_path = root.to_path_buf();
    move |relative: String, action: u32| {
        if cancelled.load(Ordering::Acquire) {
            return;
        }
        // One startup config read per root, on the watcher thread after its
        // OS subscription is active. It configures listing folds only.
        let relevance = relevance.get_or_insert_with(|| {
            let excludes =
                super::git_relevance::explorer_excludes(&root_path, Arc::clone(&cancelled));
            NoticeRelevance::new(&root_path).with_excludes(excludes)
        });
        // Retain the exact operand before Explorer folds it to its ignored
        // ancestor. The matcher supplies no Git relevance verdict.
        let original = relative.clone();
        let directory = root_path.join(&relative).is_dir();
        let moved = matches!(super::kind_for_action(action), Some("created" | "deleted"));
        let (relative, action) = if action == OVERFLOW_NOTIFY_ACTION {
            relevance.forget();
            (relative, action)
        } else {
            match relevance.classify(&relative, action) {
                Relevance::Keep => (relative, action),
                Relevance::Drop => return,
                Relevance::Resync => (
                    super::OVERFLOW_RELATIVE_PATH.to_string(),
                    OVERFLOW_NOTIFY_ACTION,
                ),
                Relevance::IgnoredBelow(folder) => (folder, IGNORED_BELOW_NOTIFY_ACTION),
            }
        };
        if let Ok(Some(notice)) =
            deliver_workspace_notify_action(&project_id, generation, &root_id, &relative, action)
        {
            sink.push_record(notice, &original, directory, moved);
        }
    }
}

pub(super) fn workspace_notify_bound_roots(project_id: &str) -> Option<BTreeSet<String>> {
    registry()
        .watches
        .get(project_id)
        .map(|watch| watch.session.roots.clone())
}

fn watch_cancel_flag(project_id: &str, generation: u32) -> Option<Arc<AtomicBool>> {
    registry()
        .watches
        .get(project_id)
        .and_then(|watch| (watch.generation == generation).then(|| Arc::clone(&watch.cancelled)))
}

fn resolve_bound_root_paths(
    database: &Database,
    project_id: &str,
    root_ids: &BTreeSet<String>,
) -> Result<Vec<(String, PathBuf)>, WorkspaceNotifyError> {
    if database.active_project_id().ok().flatten().as_deref() != Some(project_id) {
        return Err(reject("notify-stale"));
    }
    let mut paths = Vec::new();
    for root_id in root_ids {
        let record = database
            .root_record(root_id)
            .map_err(|_| reject("notify-substituted"))?;
        let path = authenticate_workspace_notify_root(
            project_id,
            root_id,
            &record.project_id,
            &record.absolute_path,
        )?;
        paths.push((root_id.clone(), path));
    }
    Ok(paths)
}

pub(super) fn start_workspace_notify_watchers(
    app: &AppHandle,
    state: &AppState,
    project_id: &str,
    generation: u32,
    root_ids: &BTreeSet<String>,
) -> Result<(), WorkspaceNotifyError> {
    let paths = resolve_bound_root_paths(&state.database, project_id, root_ids)?;
    let Some(cancelled) = watch_cancel_flag(project_id, generation) else {
        return Err(reject("notify-substituted"));
    };
    let sink = Arc::new(NoticeSink::default());
    let emitter = app.clone();
    spawn_notice_emitter(
        Arc::clone(&sink),
        Arc::clone(&cancelled),
        paths.clone(),
        move |batch| {
            let _ = emitter.emit(WORKSPACE_NOTIFY_EVENT, batch);
        },
    )
    .map_err(|_| reject("notify-substituted"))?;
    for (root_id, path) in paths {
        spawn_workspace_root_watch(
            Arc::clone(&sink),
            project_id.to_string(),
            generation,
            root_id,
            path,
            Arc::clone(&cancelled),
        )?;
    }
    Ok(())
}

#[cfg(windows)]
fn spawn_workspace_root_watch(
    sink: Arc<NoticeSink>,
    project_id: String,
    generation: u32,
    root_id: String,
    path: PathBuf,
    cancelled: Arc<AtomicBool>,
) -> Result<(), WorkspaceNotifyError> {
    windows::spawn_workspace_root_watch(sink, project_id, generation, root_id, path, cancelled)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn spawn_workspace_root_watch(
    sink: Arc<NoticeSink>,
    project_id: String,
    generation: u32,
    root_id: String,
    path: PathBuf,
    cancelled: Arc<AtomicBool>,
) -> Result<(), WorkspaceNotifyError> {
    super::unix::spawn_workspace_root_watch(sink, project_id, generation, root_id, path, cancelled)
}

#[cfg(all(unix, not(any(target_os = "macos", target_os = "linux"))))]
fn spawn_workspace_root_watch(
    _sink: Arc<NoticeSink>,
    _project_id: String,
    _generation: u32,
    _root_id: String,
    _path: PathBuf,
    _cancelled: Arc<AtomicBool>,
) -> Result<(), WorkspaceNotifyError> {
    Err(reject("notify-unavailable"))
}

#[cfg(not(any(windows, unix)))]
fn spawn_workspace_root_watch(
    _sink: Arc<NoticeSink>,
    _project_id: String,
    _generation: u32,
    _root_id: String,
    _path: PathBuf,
    _cancelled: Arc<AtomicBool>,
) -> Result<(), WorkspaceNotifyError> {
    Err(reject("notify-unavailable"))
}
