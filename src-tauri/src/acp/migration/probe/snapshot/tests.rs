use super::*;

struct TestDir(PathBuf);

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn interrupted_scratch_is_reused_for_current_wal_data_and_source_changes_are_rejected() {
    for prefix in ["complete", "partial-main", "empty"] {
        let directory = TestDir(
            crate::test_paths::temp_dir().join(format!("sage-snapshot-{}", uuid::Uuid::new_v4())),
        );
        fs::create_dir_all(&directory.0).unwrap();
        let paths = MigrationPaths::new(&directory.0.join("sage.db")).unwrap();
        let _lock =
            crate::acp::migration::platform::MigrationLock::acquire(paths.parent()).unwrap();
        let writer = Connection::open(&paths.live).unwrap();
        writer
            .execute_batch(
                "CREATE TABLE marker(value INTEGER);
                 INSERT INTO marker VALUES(1);
                 PRAGMA journal_mode=WAL;
                 PRAGMA wal_autocheckpoint=0;
                 UPDATE marker SET value=2;",
            )
            .unwrap();
        let interrupted = LiveSnapshot::capture(&paths).unwrap();
        let slot = interrupted.directory.clone();
        if prefix == "empty" {
            drop(interrupted);
            create_private_directory(&slot).unwrap();
        } else {
            if prefix == "partial-main" {
                fs::OpenOptions::new()
                    .write(true)
                    .open(interrupted.main())
                    .unwrap()
                    .set_len(64)
                    .unwrap();
                fs::remove_file(interrupted.wal()).unwrap();
            }
            // Simulate process loss: the files survive, but no handles do.
            std::mem::forget(interrupted);
        }
        writer.execute_batch("UPDATE marker SET value=3;").unwrap();

        let snapshot = LiveSnapshot::capture(&paths).unwrap();
        assert_eq!(snapshot.directory, slot);
        assert_eq!(
            snapshot
                .inspect(|connection| {
                    Ok(connection.query_row::<i64, _, _>(
                        "SELECT value FROM marker",
                        [],
                        |row| row.get(0),
                    )?)
                })
                .unwrap(),
            3
        );
        snapshot.verify_source().unwrap();
        writer.execute_batch("UPDATE marker SET value=4;").unwrap();
        assert!(snapshot.verify_source().is_err());
        drop(snapshot);
        assert!(!slot.exists());
    }
}
