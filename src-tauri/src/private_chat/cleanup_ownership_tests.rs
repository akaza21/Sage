use super::*;
use crate::project_registry::{ProjectRegistryStore, create::ProjectCreateStore};

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
