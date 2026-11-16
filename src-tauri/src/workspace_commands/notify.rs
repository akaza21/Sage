//! The `workspace-notify-v2` watch commands. The watcher itself is in
//! `workspace_notify`; these read the bound roots from the Project registry
//! and emit each batch of notices to the WebView.

use std::{collections::BTreeSet, path::PathBuf, sync::Arc};

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, State};

use crate::project_registry::ProjectRegistryStore;
use crate::state::AppState;
use crate::workspace_notify::{
    WORKSPACE_NOTIFY_EVENT, WorkspaceNotifyError, apply_unwatch_workspace_notify, reject,
    watch::authenticate_workspace_notify_root,
};
use sage_db::database::Database;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceNotifyWatchAck {
    pub generation: u32,
}

#[tauri::command]
pub fn watch_workspace_notify(
    app: AppHandle,
    state: State<'_, AppState>,
    value: Value,
) -> Result<WorkspaceNotifyWatchAck, String> {
    let generation = crate::workspace_notify::watch_workspace_notify(
        &value,
        |project_id, root_ids| resolve_bound_root_paths(&state.database, project_id, root_ids),
        Arc::new(move |batch| {
            let _ = app.emit(WORKSPACE_NOTIFY_EVENT, batch);
        }),
    )
    .map_err(|error| error.code().to_string())?;
    Ok(WorkspaceNotifyWatchAck { generation })
}

#[tauri::command]
pub fn unwatch_workspace_notify(value: Value) -> Result<(), String> {
    apply_unwatch_workspace_notify(&value).map_err(|error| error.code().to_string())
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
