use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use tauri::State;

use crate::{
    domain::ResourceRef,
    error::{AppError, AppResult},
    state::AppState,
};

#[allow(dead_code)]
pub const EDITOR_EXTERNAL_WATCH_EVENT: &str = "editor-external-watch-v1";

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

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EditorExternalWatchKind {
    Changed,
    Deleted,
}

#[allow(dead_code)]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorExternalWatchNotice {
    pub resource: ResourceRef,
    pub generation: u32,
    pub kind: EditorExternalWatchKind,
}

static WATCHES: OnceLock<Mutex<HashMap<WatchKey, u32>>> = OnceLock::new();
static GENERATIONS: OnceLock<Mutex<HashMap<WatchKey, u32>>> = OnceLock::new();

fn watches() -> std::sync::MutexGuard<'static, HashMap<WatchKey, u32>> {
    WATCHES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn generations() -> std::sync::MutexGuard<'static, HashMap<WatchKey, u32>> {
    GENERATIONS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[allow(dead_code)]
pub fn watch_event_name() -> &'static str {
    EDITOR_EXTERNAL_WATCH_EVENT
}

#[allow(dead_code)]
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
    let mut watches = watches();
    let key = WatchKey::from_resource(resource);
    if watches.contains_key(&key) {
        return Err(AppError::InvalidRequest(
            "a watch is already active for this resource".into(),
        ));
    }
    let mut generations = generations();
    let next = generations
        .get(&key)
        .copied()
        .unwrap_or(0)
        .saturating_add(1);
    generations.insert(key.clone(), next);
    watches.insert(key, next);
    Ok(next)
}

pub fn stop_watch(resource: &ResourceRef) -> AppResult<()> {
    watches().remove(&WatchKey::from_resource(resource));
    Ok(())
}

#[tauri::command]
pub fn watch_editor_document(
    _state: State<'_, AppState>,
    resource: ResourceRef,
) -> Result<u32, String> {
    start_watch(&resource).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn unwatch_editor_document(
    _state: State<'_, AppState>,
    resource: ResourceRef,
) -> Result<(), String> {
    stop_watch(&resource).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resource(relative_path: &str) -> ResourceRef {
        ResourceRef {
            root_id: "root-a".into(),
            relative_path: relative_path.into(),
        }
    }

    #[test]
    fn watch_is_resource_scoped_and_cancelable() {
        let file = resource("notes.md");
        let sibling = resource("other.md");
        watches().clear();
        generations().clear();
        assert_eq!(start_watch(&file).unwrap(), 1);
        assert!(start_watch(&file).is_err());
        assert_eq!(start_watch(&sibling).unwrap(), 1);
        stop_watch(&file).unwrap();
        assert_eq!(start_watch(&file).unwrap(), 2);
        stop_watch(&file).unwrap();
        stop_watch(&sibling).unwrap();
        assert_eq!(EDITOR_EXTERNAL_WATCH_EVENT, "editor-external-watch-v1");
        let notice = EditorExternalWatchNotice {
            resource: file,
            generation: 1,
            kind: EditorExternalWatchKind::Changed,
        };
        assert_eq!(notice.kind, EditorExternalWatchKind::Changed);
        assert!(
            serde_json::from_str::<EditorExternalWatchNotice>(
                r#"{"resource":{"rootId":"root-a","relativePath":"notes.md"},"generation":1,"kind":"renamed"}"#
            )
            .is_err()
        );
    }

    #[test]
    fn watch_generation_is_monotonic_across_rewatch() {
        let file = resource("journal.md");
        watches().clear();
        generations().clear();
        assert_eq!(start_watch(&file).unwrap(), 1);
        stop_watch(&file).unwrap();
        assert_eq!(start_watch(&file).unwrap(), 2);
        stop_watch(&file).unwrap();
        assert_eq!(start_watch(&file).unwrap(), 3);
        stop_watch(&file).unwrap();
    }
}
