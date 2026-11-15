use super::*;

#[test]
fn no_download_folder_is_saved_until_one_is_set() {
    let store = SettingsStore::in_memory().unwrap();
    assert_eq!(
        store.browser_download_folder_record().unwrap(),
        DownloadFolderRecord::default()
    );
    store.clear_browser_download_folder().unwrap();
    assert_eq!(store.browser_download_folder_record().unwrap().path, None);
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
        reopened.browser_download_folder_record().unwrap().path,
        Some(directory.clone())
    );
    reopened.clear_browser_download_folder().unwrap();
    assert_eq!(
        SettingsStore::open(&path)
            .unwrap()
            .browser_download_folder_record()
            .unwrap()
            .path,
        None
    );
    drop(reopened);
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
    let read = store.browser_download_folder_record().unwrap();
    store.set_browser_download_folder(&settings).unwrap();
    store.clear_browser_download_folder().unwrap();
    assert!(
        !store
            .set_browser_download_folder_if(read.revision, &dialog)
            .unwrap()
    );
    assert_eq!(store.browser_download_folder_record().unwrap().path, None);
    let read = store.browser_download_folder_record().unwrap();
    assert!(
        store
            .set_browser_download_folder_if(read.revision, &dialog)
            .unwrap()
    );
    assert_eq!(
        store.browser_download_folder_record().unwrap().path,
        Some(dialog)
    );
}

#[test]
fn tables_from_earlier_development_builds_are_migrated() {
    let directory = std::env::temp_dir().join(format!(
        "sage-settings-download-migrate-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("settings.db");
    {
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE browser_download_folder (
                   singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                   path TEXT NOT NULL CHECK (length(path) BETWEEN 1 AND 4096)
                 ) STRICT;
                 INSERT INTO browser_download_folder VALUES (1, '/saved');
                 CREATE TABLE browser_download_reservations (
                   path TEXT PRIMARY KEY CHECK (length(path) BETWEEN 1 AND 4096)
                 ) STRICT;
                 INSERT INTO browser_download_reservations VALUES ('/saved/a.bin');",
            )
            .unwrap();
    }
    let store = SettingsStore::open(&path).unwrap();
    assert_eq!(
        store.browser_download_folder_record().unwrap(),
        DownloadFolderRecord {
            path: Some(PathBuf::from("/saved")),
            revision: 1
        }
    );
    let connection = store.0.lock().unwrap();
    assert!(!has_table(&connection, "browser_download_reservations").unwrap());
    drop(connection);
    store.clear_browser_download_folder().unwrap();
    assert_eq!(store.browser_download_folder_record().unwrap().revision, 2);
    drop(store);
    assert_eq!(
        SettingsStore::open(&path)
            .unwrap()
            .browser_download_folder_record()
            .unwrap()
            .revision,
        2
    );
    std::fs::remove_dir_all(directory).unwrap();
}
