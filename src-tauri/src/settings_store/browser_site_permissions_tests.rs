use super::*;

fn temp_store_path(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let directory = std::env::temp_dir().join(format!("sage-{name}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("settings.db");
    (directory, path)
}

#[test]
fn site_permissions_start_empty_and_survive_reopening_the_store() {
    let (directory, path) = temp_store_path("site-permissions");
    {
        let store = SettingsStore::open(&path).unwrap();
        assert!(store.browser_site_permissions().unwrap().is_empty());
        store
            .put_browser_site_permission(
                "https://b.example",
                PermissionKind::Camera,
                Decision::Deny,
            )
            .unwrap();
        store
            .put_browser_site_permission(
                "https://a.example",
                PermissionKind::Geolocation,
                Decision::Allow,
            )
            .unwrap();
    }
    assert_eq!(
        SettingsStore::open(&path)
            .unwrap()
            .browser_site_permissions()
            .unwrap(),
        vec![
            (
                "https://a.example".to_owned(),
                PermissionKind::Geolocation,
                Decision::Allow
            ),
            (
                "https://b.example".to_owned(),
                PermissionKind::Camera,
                Decision::Deny
            ),
        ]
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn changing_and_deleting_affect_only_one_origin_and_kind() {
    let store = SettingsStore::in_memory().unwrap();
    for kind in [PermissionKind::Camera, PermissionKind::Microphone] {
        store
            .put_browser_site_permission("https://a.example", kind, Decision::Allow)
            .unwrap();
    }
    store
        .put_browser_site_permission("https://a.example", PermissionKind::Camera, Decision::Deny)
        .unwrap();
    assert!(
        store
            .delete_browser_site_permission("https://a.example", PermissionKind::Microphone)
            .unwrap()
    );
    assert!(
        !store
            .delete_browser_site_permission("https://a.example", PermissionKind::Microphone)
            .unwrap()
    );
    assert_eq!(
        store.browser_site_permissions().unwrap(),
        vec![(
            "https://a.example".to_owned(),
            PermissionKind::Camera,
            Decision::Deny
        )]
    );
    store.clear_browser_site_permissions().unwrap();
    assert!(store.browser_site_permissions().unwrap().is_empty());
}

#[test]
fn a_new_row_past_the_cap_is_refused_but_an_existing_row_still_changes() {
    let store = SettingsStore::in_memory().unwrap();
    for index in 0..MAX_SITE_PERMISSIONS {
        store
            .put_browser_site_permission(
                &format!("https://site{index}.example"),
                PermissionKind::Notifications,
                Decision::Deny,
            )
            .unwrap();
    }
    assert!(matches!(
        store.put_browser_site_permission(
            "https://one-more.example",
            PermissionKind::Notifications,
            Decision::Deny,
        ),
        Err(AppError::ResourceLimit(_))
    ));
    store
        .put_browser_site_permission(
            "https://site0.example",
            PermissionKind::Notifications,
            Decision::Allow,
        )
        .unwrap();
    let saved = store.browser_site_permissions().unwrap();
    assert_eq!(saved.len(), MAX_SITE_PERMISSIONS);
    assert!(saved.contains(&(
        "https://site0.example".to_owned(),
        PermissionKind::Notifications,
        Decision::Allow
    )));
}

#[test]
fn the_table_admits_only_known_kinds_decisions_and_bounded_origins() {
    let store = SettingsStore::in_memory().unwrap();
    assert!(
        store
            .put_browser_site_permission(
                &format!("https://{}.example", "a".repeat(MAX_SITE_ORIGIN_BYTES)),
                PermissionKind::Camera,
                Decision::Allow,
            )
            .is_err()
    );
    let connection = store.0.lock().unwrap();
    for (origin, kind, decision) in [
        ("https://a.example", "usb", "allow"),
        ("https://a.example", "camera", "ask"),
        ("http://a", "camera", "maybe"),
        ("short", "camera", "allow"),
    ] {
        assert!(
            connection
                .execute(
                    "INSERT INTO browser_site_permissions(origin, kind, decision) VALUES (?1, ?2, ?3)",
                    params![origin, kind, decision],
                )
                .is_err(),
            "{origin} {kind} {decision}"
        );
    }
}

#[test]
fn a_settings_store_from_before_site_permissions_gains_the_table() {
    let (directory, path) = temp_store_path("site-permissions-migrate");
    {
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE browser_preferences (
                   singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                   show_full_url INTEGER NOT NULL CHECK (show_full_url IN (0, 1))
                 ) STRICT;
                 INSERT INTO browser_preferences VALUES (1, 1);",
            )
            .unwrap();
    }
    let store = SettingsStore::open(&path).unwrap();
    assert!(store.browser_site_permissions().unwrap().is_empty());
    assert!(store.browser_preferences().unwrap().show_full_url);
    drop(store);
    std::fs::remove_dir_all(directory).unwrap();
}
