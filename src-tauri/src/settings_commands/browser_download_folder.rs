//! None of these commands takes a path: Change opens a native folder picker in
//! Rust, so neither the renderer, a page nor the Agent can point downloads
//! somewhere else.

use std::path::{Path, PathBuf};

use sage_core::error::AppResult;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;

use crate::{
    browser::downloads::{
        folder::admit_folder_before,
        io::{self, Deadline, Executor},
    },
    settings_store::{DownloadFolderRecord, SettingsStore},
    state::AppState,
};

/// `revision` orders this snapshot against replies and events even when the
/// availability probe is slow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DownloadFolderView {
    pub(crate) path: Option<String>,
    pub(crate) available: bool,
    pub(crate) revision: i64,
}

pub(crate) async fn folder_view(store: &SettingsStore) -> AppResult<DownloadFolderView> {
    let stored = store.browser_download_folder_record()?;
    Ok(view_with_probe(stored, io::executor(), admit_folder_before).await)
}

async fn view_with_probe(
    stored: DownloadFolderRecord,
    executor: &Executor,
    probe: impl FnOnce(&Path, Deadline) -> Option<PathBuf> + Send + 'static,
) -> DownloadFolderView {
    let available = if let Some(path) = stored.path.clone() {
        executor.probe(path, probe).await.is_some()
    } else {
        false
    };
    DownloadFolderView {
        available,
        path: stored.path.map(|path| path.to_string_lossy().into_owned()),
        revision: stored.revision,
    }
}

pub(crate) async fn remember_folder(store: &SettingsStore, chosen: &Path) -> Result<(), String> {
    remember_with_probe(store, chosen, io::executor(), admit_folder_before).await
}

async fn remember_with_probe(
    store: &SettingsStore,
    chosen: &Path,
    executor: &Executor,
    probe: impl FnOnce(&Path, Deadline) -> Option<PathBuf> + Send + 'static,
) -> Result<(), String> {
    let revision = store
        .browser_download_folder_record()
        .map_err(|error| error.to_string())?
        .revision;
    let folder = executor
        .probe(chosen.to_owned(), probe)
        .await
        .ok_or("That folder can't be used for downloads. Choose a folder you can write to.")?;
    if !store
        .set_browser_download_folder_if(revision, &folder)
        .map_err(|error| error.to_string())?
    {
        return Err(
            "The download folder changed while checking it. Choose the folder again.".into(),
        );
    }
    Ok(())
}

/// Skipped when the stored folder changed since the download read it at
/// `revision` (Settings chose or reset one while the dialog was open).
// Called by the WebView2 download handler, which only Windows builds.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) async fn remember_first_choice(
    store: &SettingsStore,
    revision: i64,
    chosen: &Path,
) -> bool {
    let Some(folder) = chosen.parent() else {
        return false;
    };
    io::executor()
        .probe(folder.to_owned(), admit_folder_before)
        .await
        .is_some_and(|folder| {
            store
                .set_browser_download_folder_if(revision, &folder)
                .unwrap_or(false)
        })
}

pub(crate) fn emit_folder_changed(app: &AppHandle, store: &SettingsStore) {
    // Every caller has just admitted and saved the folder. Announce that
    // value without starting another filesystem probe or delayed event.
    if let Ok(stored) = store.browser_download_folder_record() {
        let view = DownloadFolderView {
            available: stored.path.is_some(),
            path: stored.path.map(|path| path.to_string_lossy().into_owned()),
            revision: stored.revision,
        };
        let _ = app.emit_to("main", FOLDER_CHANGED, view);
    }
}

pub(crate) const FOLDER_CHANGED: &str = "browser:download-folder";

#[tauri::command]
pub(crate) async fn browser_download_folder(
    state: State<'_, AppState>,
) -> Result<DownloadFolderView, String> {
    folder_view(&state.settings)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) async fn browser_choose_download_folder(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<DownloadFolderView, String> {
    let view = folder_view(&state.settings)
        .await
        .map_err(|error| error.to_string())?;
    let start = view.path.filter(|_| view.available);
    let mut dialog = app.dialog().file().set_title("Choose download folder");
    if let Some(window) = app.get_window("main") {
        dialog = dialog.set_parent(&window);
    }
    if let Some(start) = start {
        dialog = dialog.set_directory(start);
    }
    let picked = tauri::async_runtime::spawn_blocking(move || dialog.blocking_pick_folder())
        .await
        .map_err(|error| error.to_string())?;
    if let Some(path) = picked.and_then(|file| file.into_path().ok()) {
        remember_folder(&state.settings, &path).await?;
        emit_folder_changed(&app, &state.settings);
    }
    folder_view(&state.settings)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) async fn browser_reset_download_folder(
    state: State<'_, AppState>,
) -> Result<DownloadFolderView, String> {
    reset_folder(&state.settings, io::executor()).map_err(|error| error.to_string())?;
    folder_view(&state.settings)
        .await
        .map_err(|error| error.to_string())
}

fn reset_folder(store: &SettingsStore, executor: &Executor) -> AppResult<()> {
    executor.cancel_probes();
    store.clear_browser_download_folder()
}

#[cfg(test)]
mod tests;
