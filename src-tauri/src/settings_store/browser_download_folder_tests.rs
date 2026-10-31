use super::*;

#[test]
fn no_download_folder_is_saved_until_one_is_set() {
    let store = SettingsStore::in_memory().unwrap();
    assert_eq!(
        store.browser_download_folder_record().unwrap(),
        DownloadFolderRecord::default()
    );
    store.clear_browser_download_folder().unwrap();
    assert_eq!(store.browser_download_folder().unwrap(), None);
}

#[test]
fn the_download_folder_persists_until_reset() {
    let directory =
        std::env::temp_dir().join(format!("sage-settings-download-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("settings.db");
    SettingsStore::open(&path)
        .unwrap()
        .set_browser_download_folder(&directory)
        .unwrap();
    let reopened = SettingsStore::open(&path).unwrap();
    assert_eq!(
        reopened.browser_download_folder().unwrap(),
        Some(directory.clone())
    );
    reopened.clear_browser_download_folder().unwrap();
    assert_eq!(
        SettingsStore::open(&path)
            .unwrap()
            .browser_download_folder()
            .unwrap(),
        None
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn a_relative_or_oversized_download_folder_is_refused() {
    let store = SettingsStore::in_memory().unwrap();
    assert!(
        store
            .set_browser_download_folder(Path::new("relative/downloads"))
            .is_err()
    );
    let long = std::env::temp_dir().join("x".repeat(MAX_PATH_BYTES + 1));
    assert!(store.set_browser_download_folder(&long).is_err());
    let connection = store.0.lock().unwrap();
    for sql in [
        "INSERT INTO browser_download_folder VALUES (1, '', 0)",
        "INSERT INTO browser_download_folder VALUES (2, '/x', 0)",
        "INSERT INTO browser_download_folder VALUES (1, '/x', -1)",
        "INSERT INTO browser_download_reservations VALUES ('')",
    ] {
        assert!(connection.execute(sql, []).is_err(), "{sql}");
    }
}

#[test]
fn every_set_and_reset_advances_the_revision() {
    let store = SettingsStore::in_memory().unwrap();
    store
        .set_browser_download_folder(&std::env::temp_dir().join("one"))
        .unwrap();
    assert_eq!(store.browser_download_folder_record().unwrap().revision, 1);
    store.clear_browser_download_folder().unwrap();
    assert_eq!(
        store.browser_download_folder_record().unwrap(),
        DownloadFolderRecord {
            path: None,
            revision: 2
        }
    );
}

#[test]
fn a_conditional_save_loses_to_any_change_since_it_read_even_a_change_back() {
    let store = SettingsStore::in_memory().unwrap();
    let dialog = std::env::temp_dir().join("dialog");
    let settings = std::env::temp_dir().join("settings");
    // The first dialog read "nothing saved"; Settings chose B, then Reset.
    let read = store.browser_download_folder_record().unwrap();
    store.set_browser_download_folder(&settings).unwrap();
    store.clear_browser_download_folder().unwrap();
    assert!(
        !store
            .set_browser_download_folder_if(read.revision, &dialog)
            .unwrap()
    );
    assert_eq!(store.browser_download_folder().unwrap(), None);
    let read = store.browser_download_folder_record().unwrap();
    assert!(
        store
            .set_browser_download_folder_if(read.revision, &dialog)
            .unwrap()
    );
    assert_eq!(store.browser_download_folder().unwrap(), Some(dialog));
}

#[test]
fn reservations_are_recorded_bounded_and_taken_once() {
    let store = SettingsStore::in_memory().unwrap();
    let base = std::env::temp_dir();
    for index in 0..MAX_RESERVATIONS {
        assert!(
            store
                .record_download_reservation(&base.join(format!("r{index}")))
                .unwrap()
        );
    }
    assert!(
        !store
            .record_download_reservation(&base.join("over"))
            .unwrap()
    );
    store.forget_download_reservation(&base.join("r0")).unwrap();
    let taken = store.take_download_reservations().unwrap();
    assert_eq!(taken.len() as i64, MAX_RESERVATIONS - 1);
    assert!(!taken.contains(&base.join("r0")));
    assert!(store.take_download_reservations().unwrap().is_empty());
}
