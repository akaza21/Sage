use super::*;
use crate::browser::downloads::folder::admit_folder;
use std::{sync::Arc, time::Duration};

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
        &Executor::new(Duration::from_secs(2)),
        move |path, deadline| {
            changed.clear_browser_download_folder().unwrap();
            admit_folder_before(path, deadline)
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
        &Executor::new(Duration::from_millis(20)),
        move |path, _| {
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
    assert!(!remember_first_choice(&store, read.revision, &dialog.join("file.zip")).await);
    assert_eq!(store.browser_download_folder_record().unwrap().path, None);
    let read = store.browser_download_folder_record().unwrap();
    assert!(remember_first_choice(&store, read.revision, &dialog.join("file.zip")).await);
    assert_eq!(
        store.browser_download_folder_record().unwrap().path,
        admit_folder(&dialog)
    );
    std::fs::remove_dir_all(dialog).unwrap();
    std::fs::remove_dir_all(settings).unwrap();
}

#[tokio::test]
async fn timed_out_probes_release_capacity_for_a_healthy_destination() {
    let executor = Arc::new(Executor::new(Duration::from_secs(1)));
    let store = SettingsStore::in_memory().unwrap();
    let directory = scratch("isolated-selection");
    let remote = directory.join("remote");
    store.set_browser_download_folder(&remote).unwrap();
    let stored = store.browser_download_folder_record().unwrap();
    let (release, wait) = std::sync::mpsc::sync_channel::<()>(0);
    let (started, start) = tokio::sync::oneshot::channel();
    let io = Arc::clone(&executor);
    let record = stored.clone();
    let first = tokio::spawn(async move {
        view_with_probe(record, &io, move |path, _| {
            started.send(()).ok();
            let _ = wait.recv_timeout(Duration::from_secs(30));
            Some(path.to_owned())
        })
        .await
    });
    start.await.unwrap();
    let second = view_with_probe(stored, &executor, admit_folder_before).await;
    assert!(!first.await.unwrap().available);
    assert!(!second.available);
    remember_with_probe(&store, &directory, &executor, admit_folder_before)
        .await
        .unwrap();
    let saved = view_with_probe(
        store.browser_download_folder_record().unwrap(),
        &executor,
        admit_folder_before,
    )
    .await;
    assert!(saved.available);
    assert_eq!(
        saved.path.as_deref().map(Path::new),
        admit_folder(&directory).as_deref()
    );
    assert_eq!(saved.revision, 2);
    drop(release);
    std::fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn reset_releases_pending_selection_without_waiting_for_the_os() {
    let executor = Arc::new(Executor::new(Duration::from_secs(30)));
    let store = SettingsStore::in_memory().unwrap();
    let directory = scratch("reset-pending");
    let remote = directory.join("remote");
    let (release, wait) = std::sync::mpsc::sync_channel::<()>(0);
    let (started, start) = tokio::sync::oneshot::channel();
    let (finished, finish) = tokio::sync::oneshot::channel();
    let io = Arc::clone(&executor);
    let pending_store = store.clone();
    let selection = tokio::spawn(async move {
        remember_with_probe(&pending_store, &remote, &io, move |path, _| {
            started.send(()).ok();
            let _ = wait.recv_timeout(Duration::from_secs(30));
            finished.send(()).ok();
            Some(path.to_owned())
        })
        .await
    });
    start.await.unwrap();
    reset_folder(&store, &executor).unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(2), selection)
            .await
            .expect("reset must release a pending selection before its deadline")
            .unwrap()
            .is_err()
    );
    let reset = folder_view(&store).await.unwrap();
    assert_eq!(reset.path, None);
    assert_eq!(reset.revision, 1);
    remember_with_probe(&store, &directory, &executor, admit_folder_before)
        .await
        .unwrap();
    drop(release);
    finish.await.unwrap();
    let view = folder_view(&store).await.unwrap();
    assert!(view.available);
    assert_eq!(
        view.path.as_deref().map(Path::new),
        admit_folder(&directory).as_deref()
    );
    assert_eq!(view.revision, 2);
    std::fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn a_selection_cannot_overwrite_a_reset_or_a_newer_choice() {
    let executor = Executor::new(Duration::from_secs(2));
    let store = SettingsStore::in_memory().unwrap();
    let directory = scratch("selection-revision");
    let selected = directory.join("selected");
    std::fs::create_dir(&selected).unwrap();
    let changed = store.clone();
    assert!(
        remember_with_probe(&store, &selected, &executor, move |path, deadline| {
            changed.clear_browser_download_folder().unwrap();
            admit_folder_before(path, deadline)
        })
        .await
        .is_err()
    );
    assert_eq!(folder_view(&store).await.unwrap().path, None);
    let changed = store.clone();
    let newer = directory.clone();
    assert!(
        remember_with_probe(&store, &selected, &executor, move |path, deadline| {
            changed.set_browser_download_folder(&newer).unwrap();
            admit_folder_before(path, deadline)
        })
        .await
        .is_err()
    );
    let view = folder_view(&store).await.unwrap();
    assert!(view.available);
    assert_eq!(view.path.as_deref(), directory.to_str());
    std::fs::remove_dir_all(directory).unwrap();
}
