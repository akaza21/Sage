use super::*;
use crate::project_registry::{ProjectRegistryStore, create::ProjectCreateStore};

#[cfg(windows)]
#[test]
fn cleanup_preserves_short_and_long_overlapping_roots_in_both_windows_spellings() {
    for long in [false, true] {
        for verbatim in [false, true] {
            let root = std::env::current_dir()
                .unwrap()
                .join("work")
                .join(format!("private-overlap-identity-{}", Uuid::new_v4()));
            fs::create_dir_all(root.join("user")).unwrap();
            let database = crate::database::open_database(&root.join("sage.db")).unwrap();
            let user = database
                .create_project_from_root(&root.join("user"))
                .unwrap();
            let chat = Uuid::new_v4().to_string();
            create(&database, None, &chat).unwrap();
            let scope = super::tests::layout(&database);
            let folder = database.root_records(&scope.project_id).unwrap()[0]
                .absolute_path
                .clone();
            assert!(folder.as_os_str().len() < 260);
            let mut registered = folder.canonicalize().unwrap();
            for _ in 0..if long { 4 } else { 1 } {
                registered.push("overlapping-project-descendant-component");
            }
            fs::create_dir_all(&registered).unwrap();
            let marker = registered.join("keep.txt");
            fs::write(&marker, "registered project work").unwrap();
            let canonical = registered.to_str().unwrap();
            let ordinary = canonical.strip_prefix(r"\\?\").unwrap();
            assert_eq!(ordinary.encode_utf16().count() > 260, long);
            let stored = if verbatim { canonical } else { ordinary };
            // Reproduce an overlapping root admitted by an earlier version,
            // including its legacy key, independently of current admission.
            database.connection().execute(
                "UPDATE project_roots SET absolute_path=?1,canonical_key=?2 WHERE project_id=?3",
                params![stored, stored.to_lowercase(), user.id],
            ).unwrap();
            super::cleanup_tests::delete_chat(&database, &chat);
            let error = cleanup::empty_projects(&database).unwrap_err();
            assert!(
                error
                    .public_message()
                    .contains("overlaps another project root")
            );
            assert_eq!(
                fs::read_to_string(&marker).unwrap(),
                "registered project work"
            );
            assert!(database.project_summary(&user.id).is_ok());
            assert!(
                database.project_summary(&scope.project_id).is_ok(),
                "retain the cleanup receipt"
            );
            drop(database);
            fs::remove_dir_all(root).unwrap();
        }
    }
}

#[cfg(windows)]
#[test]
fn long_private_chat_descendant_cannot_be_registered_as_a_project_root() {
    let root = std::env::current_dir()
        .unwrap()
        .join("work")
        .join(format!("private-long-registration-{}", Uuid::new_v4()));
    fs::create_dir_all(root.join("user")).unwrap();
    let database = crate::database::open_database(&root.join("sage.db")).unwrap();
    let user = database
        .create_project_from_root(&root.join("user"))
        .unwrap();
    create(&database, None, &Uuid::new_v4().to_string()).unwrap();
    let scope = super::tests::layout(&database);
    let folder = database.root_records(&scope.project_id).unwrap()[0]
        .absolute_path
        .clone();
    assert!(folder.as_os_str().len() < 260);
    let mut descendant = folder.canonicalize().unwrap();
    for _ in 0..4 {
        descendant.push("long-descendant-component-for-registration");
    }
    fs::create_dir_all(&descendant).unwrap();
    assert!(descendant.as_os_str().len() > 264);
    let result = database.create_project_from_root(&descendant);
    assert!(
        result.is_err(),
        "a long managed descendant must remain private"
    );
    assert!(
        result
            .unwrap_err()
            .public_message()
            .contains("managed private Chat")
    );
    assert!(
        database
            .create_named_project_if_active(
                "Unsafe",
                std::slice::from_ref(&descendant),
                Some(&scope.project_id)
            )
            .unwrap_err()
            .public_message()
            .contains("managed private Chat")
    );
    database
        .connection()
        .execute("UPDATE app_state SET active_project_id=?1", [&user.id])
        .unwrap();
    assert!(
        database
            .add_project_root(&user.id, &descendant)
            .unwrap_err()
            .public_message()
            .contains("managed private Chat")
    );
    drop(database);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn round3_cleanup_releases_sqlite_and_rechecks_its_durable_receipt_before_forgetting() {
    for change_receipt in [false, true] {
        let root = std::env::current_dir()
            .unwrap()
            .join("work")
            .join(format!("private-cleanup-claim-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("user")).unwrap();
        let database = crate::database::open_database(&root.join("sage.db")).unwrap();
        let user = database
            .create_project_from_root(&root.join("user"))
            .unwrap();
        let id = Uuid::new_v4().to_string();
        let folder = storage_root(&database).unwrap().join(&id);
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("stale.txt"), "reclaim after deletion").unwrap();
        database.connection().execute("INSERT INTO projects(id,name,kind,created_at,updated_at,last_opened_at) VALUES (?1,'Choose project','privateChat',0,0,0)", [&id]).unwrap();
        let probe = database.connection_ownership_probe().upgrade().unwrap();
        let storage = lock_storage();
        cleanup::empty_projects_with_storage(&database, &storage, |_| {
            // Execute a real concurrent durable write at the filesystem seam.
            // try_lock makes a retained SQLite mutex a deterministic failure.
            let connection = probe
                .try_lock()
                .expect("filesystem cleanup must release SQLite");
            connection
                .execute(
                    "UPDATE projects SET name='Changed during cleanup' WHERE id=?1",
                    [&user.id],
                )
                .unwrap();
            if change_receipt {
                connection
                    .execute(
                        "UPDATE projects SET updated_at=updated_at+1 WHERE id=?1",
                        [&id],
                    )
                    .unwrap();
            }
        })
        .unwrap();
        drop(storage);
        assert!(!folder.exists());
        assert_eq!(
            database.project_summary(&user.id).unwrap().name,
            "Changed during cleanup"
        );
        assert_eq!(database.project_summary(&id).is_ok(), change_receipt);
        cleanup::empty_projects(&database).unwrap();
        assert!(database.project_summary(&id).is_err());
        drop(probe);
        drop(database);
        fs::remove_dir_all(root).unwrap();
    }
}
