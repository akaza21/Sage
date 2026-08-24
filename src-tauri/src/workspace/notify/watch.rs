use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use tauri::AppHandle;

use crate::{persistence::Database, state::AppState};

use super::{
    bounded_identifier, registry, reject, relative_path, WorkspaceNotifyError,
    WorkspaceNotifyNotice, MAX_QUEUE,
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
    let Some(notice) = watch.session.classify(root_id, relative, action)? else {
        return Ok(None);
    };
    if watch.session.queue.len() >= MAX_QUEUE {
        return Err(reject("notify-overflow"));
    }
    watch.session.queue.push(notice.clone());
    Ok(Some(notice))
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
    for (root_id, path) in paths {
        spawn_workspace_root_watch(
            app.clone(),
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
    app: AppHandle,
    project_id: String,
    generation: u32,
    root_id: String,
    path: PathBuf,
    cancelled: Arc<AtomicBool>,
) -> Result<(), WorkspaceNotifyError> {
    windows::spawn_workspace_root_watch(app, project_id, generation, root_id, path, cancelled)
}

#[cfg(not(windows))]
fn spawn_workspace_root_watch(
    _app: AppHandle,
    _project_id: String,
    _generation: u32,
    _root_id: String,
    _path: PathBuf,
    _cancelled: Arc<AtomicBool>,
) -> Result<(), WorkspaceNotifyError> {
    Ok(())
}
