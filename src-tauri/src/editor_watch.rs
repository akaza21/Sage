use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
};

use sage_core::error::{AppError, AppResult};
use tauri::{Emitter, State};

use crate::project_registry::ProjectRegistryStore;
use crate::state::AppState;
use sage_domain::resource::ResourceRef;

pub const EDITOR_EXTERNAL_WATCH_EVENT: &str = "editor-external-watch-v1";

// Allow 32 tabs per project across 8 workspace watches, even though only active
// File canvases normally watch. This bounds native threads and handles while
// leaving headroom for every tab in those workspaces to be watched concurrently.
const MAX_GLOBAL_WATCHES: usize = 256;

/// The IPC error a File editor receives when every watch slot is taken. It is a
/// stable code rather than display text, so the editor can tell "retry later"
/// apart from a watch that cannot start at all.
pub const EDITOR_WATCH_LIMIT_ERROR: &str = "editor-watch-limit-reached";

const FILE_ACTION_ADDED: u32 = 1;
const FILE_ACTION_REMOVED: u32 = 2;
const FILE_ACTION_MODIFIED: u32 = 3;
const FILE_ACTION_RENAMED_OLD_NAME: u32 = 4;
const FILE_ACTION_RENAMED_NEW_NAME: u32 = 5;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct WatchKey {
    root_id: String,
    relative_path: String,
}

impl WatchKey {
    fn from_resource(resource: &ResourceRef) -> Self {
        Self {
            root_id: resource.root_id.clone(),
            relative_path: resource.relative_path.clone(),
        }
    }
}

#[derive(Debug)]
struct WatchEntry {
    generation: u32,
    cancelled: Arc<AtomicBool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EditorExternalWatchKind {
    Changed,
    Deleted,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorExternalWatchNotice {
    pub resource: ResourceRef,
    pub generation: u32,
    pub kind: EditorExternalWatchKind,
}

static WATCHES: OnceLock<Mutex<HashMap<WatchKey, WatchEntry>>> = OnceLock::new();
/// One counter for every watch, so a restarted watch never reuses a stale
/// generation and nothing is kept per file once its watch ends.
static NEXT_GENERATION: AtomicU32 = AtomicU32::new(0);

fn watches() -> std::sync::MutexGuard<'static, HashMap<WatchKey, WatchEntry>> {
    WATCHES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn next_generation() -> AppResult<u32> {
    NEXT_GENERATION
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            current.checked_add(1)
        })
        .map(|previous| previous + 1)
        .map_err(|_| AppError::ResourceLimit("editor file watch generations exhausted".into()))
}

#[cfg(test)]
pub fn watch_event_name() -> &'static str {
    EDITOR_EXTERNAL_WATCH_EVENT
}

pub fn watch_notice(
    resource: ResourceRef,
    generation: u32,
    kind: EditorExternalWatchKind,
) -> EditorExternalWatchNotice {
    EditorExternalWatchNotice {
        resource,
        generation,
        kind,
    }
}

pub fn start_watch(resource: &ResourceRef) -> AppResult<u32> {
    let key = WatchKey::from_resource(resource);
    let mut watches = watches();
    if watches.contains_key(&key) {
        return Err(AppError::InvalidRequest(
            "a watch is already active for this resource".into(),
        ));
    }
    if watches.len() >= MAX_GLOBAL_WATCHES {
        return Err(AppError::ResourceLimit(
            "too many active editor file watches".into(),
        ));
    }
    let generation = next_generation()?;
    watches.insert(
        key,
        WatchEntry {
            generation,
            cancelled: Arc::new(AtomicBool::new(false)),
        },
    );
    Ok(generation)
}

pub fn stop_watch(resource: &ResourceRef) -> AppResult<()> {
    if let Some(entry) = watches().remove(&WatchKey::from_resource(resource)) {
        entry.cancelled.store(true, Ordering::Release);
    }
    Ok(())
}

fn watch_control(resource: &ResourceRef, generation: u32) -> Option<Arc<AtomicBool>> {
    watches()
        .get(&WatchKey::from_resource(resource))
        .filter(|entry| entry.generation == generation)
        .map(|entry| Arc::clone(&entry.cancelled))
}

fn finish_watch(resource: &ResourceRef, generation: u32) {
    let key = WatchKey::from_resource(resource);
    let mut watches = watches();
    if watches
        .get(&key)
        .is_some_and(|entry| entry.generation == generation)
    {
        watches.remove(&key);
    }
}

fn emit_if_active(
    app: &tauri::AppHandle,
    resource: &ResourceRef,
    generation: u32,
    cancelled: &AtomicBool,
    kind: EditorExternalWatchKind,
) {
    deliver_if_active(resource, generation, cancelled, kind, |notice| {
        let _ = app.emit(EDITOR_EXTERNAL_WATCH_EVENT, notice);
    });
}

fn deliver_if_active(
    resource: &ResourceRef,
    generation: u32,
    cancelled: &AtomicBool,
    kind: EditorExternalWatchKind,
    deliver: impl FnOnce(EditorExternalWatchNotice),
) {
    if cancelled.load(Ordering::Acquire) {
        return;
    }
    let watches = watches();
    if watches
        .get(&WatchKey::from_resource(resource))
        .is_some_and(|entry| {
            entry.generation == generation && !entry.cancelled.load(Ordering::Acquire)
        })
    {
        deliver(watch_notice(resource.clone(), generation, kind));
    }
}

#[cfg(test)]
fn can_emit(resource: &ResourceRef, generation: u32, cancelled: &AtomicBool) -> bool {
    !cancelled.load(Ordering::Acquire)
        && watches()
            .get(&WatchKey::from_resource(resource))
            .is_some_and(|entry| {
                entry.generation == generation && !entry.cancelled.load(Ordering::Acquire)
            })
}

fn notice_kind_for_action(action: u32) -> Option<EditorExternalWatchKind> {
    match action {
        FILE_ACTION_REMOVED | FILE_ACTION_RENAMED_OLD_NAME => {
            Some(EditorExternalWatchKind::Deleted)
        }
        FILE_ACTION_ADDED | FILE_ACTION_MODIFIED | FILE_ACTION_RENAMED_NEW_NAME => {
            Some(EditorExternalWatchKind::Changed)
        }
        _ => None,
    }
}

#[cfg(any(test, windows))]
fn notice_kind_for_path(
    watched_file_name: &str,
    changed_file_name: &str,
    action: u32,
) -> Option<EditorExternalWatchKind> {
    watched_file_name
        .eq_ignore_ascii_case(changed_file_name)
        .then(|| notice_kind_for_action(action))
        .flatten()
}

#[cfg(any(test, target_os = "macos", target_os = "linux"))]
pub(crate) fn editor_notice_kind_from_unix_delivery(
    watched_file_name: &str,
    relative: &str,
    action: u32,
) -> Option<EditorExternalWatchKind> {
    editor_notice_kind_from_unix_delivery_case(watched_file_name, relative, action, !cfg!(windows))
}

#[cfg(any(test, target_os = "macos", target_os = "linux"))]
fn editor_notice_kind_from_unix_delivery_case(
    watched_file_name: &str,
    relative: &str,
    action: u32,
    case_sensitive: bool,
) -> Option<EditorExternalWatchKind> {
    if relative == crate::workspace_notify::OVERFLOW_RELATIVE_PATH
        && action == crate::workspace_notify::OVERFLOW_NOTIFY_ACTION
    {
        return Some(EditorExternalWatchKind::Changed);
    }
    let changed_file_name = relative.rsplit('/').next().unwrap_or(relative);
    let names_match = if case_sensitive {
        watched_file_name == changed_file_name
    } else {
        watched_file_name.eq_ignore_ascii_case(changed_file_name)
    };
    if !names_match {
        return None;
    }
    notice_kind_for_action(action)
}

#[cfg(any(test, target_os = "macos", target_os = "linux"))]
pub(crate) fn editor_watch_failure_resync(cancelled: bool) -> Option<EditorExternalWatchKind> {
    (!cancelled).then_some(EditorExternalWatchKind::Changed)
}

#[cfg(test)]
pub(crate) fn editor_unix_watch_supported() -> bool {
    cfg!(any(target_os = "macos", target_os = "linux"))
}

#[tauri::command]
pub fn watch_editor_document(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    resource: ResourceRef,
) -> Result<u32, String> {
    let path = state
        .database
        .resolve_active_resource(&resource)
        .and_then(|workspace| {
            let relative_path = workspace.validate_resource(&resource, false)?;
            workspace.resolve_existing_file(&relative_path)
        })
        .and_then(checked_file_path)
        .map_err(|error| error.to_string())?;
    let generation = start_watch(&resource).map_err(watch_command_error)?;
    let Some(cancelled) = watch_control(&resource, generation) else {
        return Err("editor watch was cancelled before it could start".into());
    };
    if let Err(error) = spawn_editor_watch(app, resource.clone(), path, generation, cancelled) {
        let _ = stop_watch(&resource);
        return Err(error.to_string());
    }
    Ok(generation)
}

#[tauri::command]
pub fn unwatch_editor_document(
    _state: State<'_, AppState>,
    resource: ResourceRef,
) -> Result<(), String> {
    stop_watch(&resource).map_err(|error| error.to_string())
}

fn watch_command_error(error: AppError) -> String {
    match error {
        AppError::ResourceLimit(_) => EDITOR_WATCH_LIMIT_ERROR.into(),
        other => other.to_string(),
    }
}

fn checked_file_path(path: PathBuf) -> AppResult<PathBuf> {
    if path.metadata()?.is_file() {
        Ok(path)
    } else {
        Err(AppError::InvalidRequest(
            "editor watches require an existing file".into(),
        ))
    }
}

#[cfg(windows)]
mod windows;

#[cfg(any(target_os = "macos", target_os = "linux"))]
mod unix;

#[cfg(windows)]
use windows::spawn_editor_watch;

#[cfg(any(target_os = "macos", target_os = "linux"))]
use unix::spawn_editor_watch;

#[cfg(all(unix, not(any(target_os = "macos", target_os = "linux"))))]
fn spawn_editor_watch(
    _app: tauri::AppHandle,
    _resource: ResourceRef,
    _path: PathBuf,
    _generation: u32,
    _cancelled: Arc<AtomicBool>,
) -> AppResult<()> {
    Err(AppError::Unsupported(
        "editor filesystem watches are unavailable on this Unix".into(),
    ))
}

#[cfg(not(any(windows, unix)))]
fn spawn_editor_watch(
    _app: tauri::AppHandle,
    _resource: ResourceRef,
    _path: PathBuf,
    _generation: u32,
    _cancelled: Arc<AtomicBool>,
) -> AppResult<()> {
    Err(AppError::Unsupported(
        "editor filesystem watches are unavailable on this platform".into(),
    ))
}

#[cfg(test)]
mod tests;
