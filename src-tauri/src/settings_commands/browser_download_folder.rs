//! Settings > Browser > Downloads. These commands run only for the main
//! webview (the invoke guard in `ipc_commands.rs` rejects browser tabs), and
//! none takes a path: Change opens a native folder picker in Rust, so neither
//! the renderer, a page nor the Agent can point downloads somewhere else.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
    time::Duration,
};

use tokio::sync::Semaphore;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;

use crate::{
    browser::downloads::folder::{admit_folder, folder_of_choice},
    error::AppResult,
    settings_store::{DownloadFolderRecord, SettingsStore},
    state::AppState,
};

/// What the Downloads row shows. `path` is the saved folder as stored;
/// `available` is false when it no longer exists or cannot be written, in
/// which case the next download asks again. `revision` orders this snapshot
/// against replies and events even when the availability probe is slow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DownloadFolderView {
    pub(crate) path: Option<String>,
    pub(crate) available: bool,
    pub(crate) revision: i64,
}

const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
static PROBES: OnceLock<Arc<Semaphore>> = OnceLock::new();

pub(crate) async fn folder_view(store: &SettingsStore) -> AppResult<DownloadFolderView> {
    let stored = store.browser_download_folder_record()?;
    Ok(view_with_probe(stored, PROBE_TIMEOUT, probe_slots(), admit_folder).await)
}

fn probe_slots() -> Arc<Semaphore> {
    PROBES.get_or_init(|| Arc::new(Semaphore::new(2))).clone()
}

async fn view_with_probe(
    stored: DownloadFolderRecord,
    timeout: Duration,
    slots: Arc<Semaphore>,
    probe: impl FnOnce(&Path) -> Option<PathBuf> + Send + 'static,
) -> DownloadFolderView {
    let available = if let Some(path) = stored.path.clone() {
        bounded_probe(path, timeout, slots, probe).await.is_some()
    } else {
        false
    };
    DownloadFolderView {
        available,
        path: stored.path.map(|path| path.to_string_lossy().into_owned()),
        revision: stored.revision,
    }
}

async fn bounded_probe(
    path: PathBuf,
    timeout: Duration,
    slots: Arc<Semaphore>,
    probe: impl FnOnce(&Path) -> Option<PathBuf> + Send + 'static,
) -> Option<PathBuf> {
    // No queue: a disconnected share retains its permit even after timeout,
    // until the OS call returns. Repeated reads cannot accumulate blocked jobs.
    let permit = slots.try_acquire_owned().ok()?;
    let work = tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        probe(&path)
    });
    tokio::time::timeout(timeout, work).await.ok()?.ok()?
}

/// Saves `chosen` as the download folder after admitting it.
pub(crate) async fn remember_folder(store: &SettingsStore, chosen: &Path) -> Result<(), String> {
    let folder = bounded_probe(
        chosen.to_owned(),
        PROBE_TIMEOUT,
        probe_slots(),
        admit_folder,
    )
    .await
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
    state
        .settings
        .clear_browser_download_folder()
        .map_err(|error| error.to_string())?;
    folder_view(&state.settings)
        .await
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

    #[tokio::test]
    async fn settings_change_saves_the_canonical_folder_and_reset_clears_it() {
        let store = SettingsStore::in_memory().unwrap();
        assert_eq!(
            folder_view(&store).await.unwrap(),
            DownloadFolderView {
                path: None,
                available: false,
                revision: 0,
            }
        );
        let directory = scratch("change");
        let nested = directory.join("inner");
        std::fs::create_dir_all(&nested).unwrap();
        // A path with `..` in it is stored in canonical form.
        remember_folder(&store, &nested.join("..").join("inner"))
            .await
            .unwrap();
        let view = folder_view(&store).await.unwrap();
        assert!(view.available);
        assert_eq!(view.revision, 1);
        assert_eq!(
            view.path.as_deref().map(Path::new),
            Some(admit_folder(&nested).unwrap().as_path())
        );
        store.clear_browser_download_folder().unwrap();
        let reset = folder_view(&store).await.unwrap();
        assert_eq!(reset.path, None);
        assert_eq!(reset.revision, 2);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn settings_change_refuses_a_missing_folder_or_a_file() {
        let store = SettingsStore::in_memory().unwrap();
        let directory = scratch("refuse");
        let file = directory.join("note.txt");
        std::fs::write(&file, b"x").unwrap();
        assert!(
            remember_folder(&store, &directory.join("missing"))
                .await
                .is_err()
        );
        assert!(remember_folder(&store, &file).await.is_err());
        assert_eq!(store.browser_download_folder_record().unwrap().path, None);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn a_saved_folder_that_was_deleted_shows_as_unavailable() {
        let store = SettingsStore::in_memory().unwrap();
        let directory = scratch("deleted");
        remember_folder(&store, &directory).await.unwrap();
        std::fs::remove_dir_all(&directory).unwrap();
        let view = folder_view(&store).await.unwrap();
        assert!(view.path.is_some());
        assert!(!view.available);
    }

    #[tokio::test]
    async fn a_probe_keeps_the_path_and_revision_it_read_together() {
        let store = SettingsStore::in_memory().unwrap();
        let directory = scratch("probe-revision");
        remember_folder(&store, &directory).await.unwrap();
        let stored = store.browser_download_folder_record().unwrap();
        let expected = stored.clone();
        let changed = store.clone();
        let view = view_with_probe(
            stored,
            Duration::from_secs(2),
            Arc::new(Semaphore::new(1)),
            move |path| {
                changed.clear_browser_download_folder().unwrap();
                admit_folder(path)
            },
        )
        .await;
        let path = view.path.as_deref().map(Path::new);
        assert_eq!(path, expected.path.as_deref());
        assert_eq!(view.revision, expected.revision);
        let reset = folder_view(&store).await.unwrap();
        assert_eq!(reset.path, None);
        assert!(reset.revision > view.revision);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn timed_out_probes_keep_the_limit_until_the_os_work_finishes() {
        let directory = scratch("probe-limit");
        let slots = Arc::new(Semaphore::new(1));
        let (release, blocked) = std::sync::mpsc::channel();
        let first = view_with_probe(
            DownloadFolderRecord {
                path: Some(directory.clone()),
                revision: 1,
            },
            Duration::from_millis(20),
            slots.clone(),
            move |path| {
                let _ = blocked.recv_timeout(Duration::from_secs(5));
                Some(path.to_owned())
            },
        )
        .await;
        let second = view_with_probe(
            DownloadFolderRecord {
                path: Some(directory.clone()),
                revision: 1,
            },
            Duration::from_millis(20),
            slots.clone(),
            |path| Some(path.to_owned()),
        )
        .await;
        release.send(()).unwrap();
        // Wait for the real blocked job to return before testing recovery.
        drop(
            tokio::time::timeout(Duration::from_secs(5), slots.acquire())
                .await
                .unwrap()
                .unwrap(),
        );
        let recovered = view_with_probe(
            DownloadFolderRecord {
                path: Some(directory.clone()),
                revision: 1,
            },
            Duration::from_secs(2),
            slots,
            |path| Some(path.to_owned()),
        )
        .await;
        assert!(!first.available);
        assert!(
            !second.available,
            "a timeout must not free an occupied probe slot"
        );
        assert!(recovered.available);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn a_slow_folder_probe_reports_unavailable_without_blocking_the_runtime() {
        let directory = scratch("slow");
        let (release, blocked) = std::sync::mpsc::channel();
        // Release even a synchronous, broken implementation so the red test
        // terminates. The async timeout must win before this fallback does.
        let fallback = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(500));
            let _ = release.send(());
        });
        let view = view_with_probe(
            DownloadFolderRecord {
                path: Some(directory.clone()),
                revision: 1,
            },
            Duration::from_millis(20),
            Arc::new(Semaphore::new(1)),
            move |path| {
                blocked.recv_timeout(Duration::from_secs(2)).unwrap();
                Some(path.to_owned())
            },
        )
        .await;
        fallback.join().unwrap();
        assert_eq!(view.path.as_deref(), directory.to_str());
        assert!(!view.available, "a slow share must time out as unavailable");
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn the_first_save_choice_is_remembered_only_if_settings_did_not_change_meanwhile() {
        let store = SettingsStore::in_memory().unwrap();
        let dialog = scratch("first-dialog");
        let settings = scratch("first-settings");
        // The download read "nothing saved"; Settings chose a folder and
        // then reset it, so the store looks unchanged but is not.
        let read = store.browser_download_folder_record().unwrap();
        remember_folder(&store, &settings).await.unwrap();
        store.clear_browser_download_folder().unwrap();
        assert!(!remember_first_choice(
            &store,
            read.revision,
            &dialog.join("file.zip")
        ));
        assert_eq!(store.browser_download_folder_record().unwrap().path, None);
        // With nothing in between, the dialog's folder is saved.
        let read = store.browser_download_folder_record().unwrap();
        assert!(remember_first_choice(
            &store,
            read.revision,
            &dialog.join("file.zip")
        ));
        assert_eq!(
            store.browser_download_folder_record().unwrap().path,
            admit_folder(&dialog)
        );
        std::fs::remove_dir_all(dialog).unwrap();
        std::fs::remove_dir_all(settings).unwrap();
    }
}
