//! Settings > Browser > Downloads. These commands run only for the main
//! webview (the invoke guard in `ipc_commands.rs` rejects browser tabs), and
//! none takes a path: Change opens a native folder picker in Rust, so neither
//! the renderer, a page nor the Agent can point downloads somewhere else.

use std::path::Path;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;

use crate::{
    browser::downloads::folder::{admit_folder, folder_of_choice},
    error::AppResult,
    settings_store::SettingsStore,
    state::AppState,
};

/// What the Downloads row shows. `path` is the saved folder as stored;
/// `available` is false when it no longer exists or cannot be written, in
/// which case the next download asks again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DownloadFolderView {
    pub(crate) path: Option<String>,
    pub(crate) available: bool,
}

pub(crate) fn folder_view(store: &SettingsStore) -> AppResult<DownloadFolderView> {
    let stored = store.browser_download_folder()?;
    Ok(DownloadFolderView {
        available: stored.as_deref().and_then(admit_folder).is_some(),
        path: stored.map(|path| path.to_string_lossy().into_owned()),
    })
}

/// Saves `chosen` as the download folder after admitting it.
pub(crate) fn remember_folder(store: &SettingsStore, chosen: &Path) -> Result<(), String> {
    let folder = admit_folder(chosen)
        .ok_or("That folder can't be used for downloads. Choose a folder you can write to.")?;
    store
        .set_browser_download_folder(&folder)
        .map_err(|error| error.to_string())
}

/// Saves the folder of the first download's Save-dialog choice, unless the
/// stored folder changed since the download read it at `revision` (Settings
/// chose or reset one while the dialog was open). Returns whether it saved.
// Called by the WebView2 download handler, which only Windows builds.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn remember_first_choice(store: &SettingsStore, revision: i64, chosen: &Path) -> bool {
    folder_of_choice(chosen).is_some_and(|folder| {
        store
            .set_browser_download_folder_if(revision, &folder)
            .unwrap_or(false)
    })
}

/// Tells an open Settings page that the download folder changed.
pub(crate) fn emit_folder_changed(app: &AppHandle, store: &SettingsStore) {
    if let Ok(view) = folder_view(store) {
        let _ = app.emit_to("main", FOLDER_CHANGED, view);
    }
}

pub(crate) const FOLDER_CHANGED: &str = "browser:download-folder";

#[tauri::command]
pub(crate) fn browser_download_folder(
    state: State<'_, AppState>,
) -> Result<DownloadFolderView, String> {
    folder_view(&state.settings).map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) async fn browser_choose_download_folder(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<DownloadFolderView, String> {
    let start = state
        .settings
        .browser_download_folder()
        .map_err(|error| error.to_string())?
        .as_deref()
        .and_then(admit_folder);
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
        remember_folder(&state.settings, &path)?;
        emit_folder_changed(&app, &state.settings);
    }
    folder_view(&state.settings).map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn browser_reset_download_folder(
    state: State<'_, AppState>,
) -> Result<DownloadFolderView, String> {
    state
        .settings
        .clear_browser_download_folder()
        .and_then(|()| folder_view(&state.settings))
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "sage-download-view-{name}-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        directory
    }

    #[test]
    fn settings_change_saves_the_canonical_folder_and_reset_clears_it() {
        let store = SettingsStore::in_memory().unwrap();
        assert_eq!(
            folder_view(&store).unwrap(),
            DownloadFolderView {
                path: None,
                available: false
            }
        );
        let directory = scratch("change");
        let nested = directory.join("inner");
        std::fs::create_dir_all(&nested).unwrap();
        // A path with `..` in it is stored in canonical form.
        remember_folder(&store, &nested.join("..").join("inner")).unwrap();
        let view = folder_view(&store).unwrap();
        assert!(view.available);
        assert_eq!(
            view.path.as_deref().map(Path::new),
            Some(admit_folder(&nested).unwrap().as_path())
        );
        store.clear_browser_download_folder().unwrap();
        assert_eq!(folder_view(&store).unwrap().path, None);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn settings_change_refuses_a_missing_folder_or_a_file() {
        let store = SettingsStore::in_memory().unwrap();
        let directory = scratch("refuse");
        let file = directory.join("note.txt");
        std::fs::write(&file, b"x").unwrap();
        assert!(remember_folder(&store, &directory.join("missing")).is_err());
        assert!(remember_folder(&store, &file).is_err());
        assert_eq!(store.browser_download_folder().unwrap(), None);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_saved_folder_that_was_deleted_shows_as_unavailable() {
        let store = SettingsStore::in_memory().unwrap();
        let directory = scratch("deleted");
        remember_folder(&store, &directory).unwrap();
        std::fs::remove_dir_all(&directory).unwrap();
        let view = folder_view(&store).unwrap();
        assert!(view.path.is_some());
        assert!(!view.available);
    }

    #[test]
    fn the_first_save_choice_is_remembered_only_if_settings_did_not_change_meanwhile() {
        let store = SettingsStore::in_memory().unwrap();
        let dialog = scratch("first-dialog");
        let settings = scratch("first-settings");
        // The download read "nothing saved"; Settings chose a folder and
        // then reset it, so the store looks unchanged but is not.
        let read = store.browser_download_folder_record().unwrap();
        remember_folder(&store, &settings).unwrap();
        store.clear_browser_download_folder().unwrap();
        assert!(!remember_first_choice(
            &store,
            read.revision,
            &dialog.join("file.zip")
        ));
        assert_eq!(store.browser_download_folder().unwrap(), None);
        // With nothing in between, the dialog's folder is saved.
        let read = store.browser_download_folder_record().unwrap();
        assert!(remember_first_choice(
            &store,
            read.revision,
            &dialog.join("file.zip")
        ));
        assert_eq!(
            store.browser_download_folder().unwrap(),
            admit_folder(&dialog)
        );
        std::fs::remove_dir_all(dialog).unwrap();
        std::fs::remove_dir_all(settings).unwrap();
    }
}
