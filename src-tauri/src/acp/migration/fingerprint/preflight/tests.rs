use super::super::MAX_SCHEMA_VERSION;
use super::*;
use crate::acp::migration::{open_or_migrate, paths::MigrationPaths};

struct TestDir(std::path::PathBuf);

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn snapshot(directory: &Path) -> Vec<(std::ffi::OsString, Vec<u8>)> {
    let mut entries = std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (entry.file_name(), std::fs::read(entry.path()).unwrap())
        })
        .collect::<Vec<_>>();
    entries.sort();
    entries
}

#[test]
fn a_profile_migrated_by_a_newer_sage_is_refused_by_name_before_recovery_touches_it() {
    refuses_newer_profile_untouched(false);
}

#[test]
fn a_checkpointed_newer_profile_is_refused_before_recovery_beside_a_nonempty_wal() {
    refuses_newer_profile_untouched(true);
}

fn refuses_newer_profile_untouched(with_wal: bool) {
    let directory = TestDir(
        crate::test_paths::temp_dir().join(format!("sage-newer-profile-{}", uuid::Uuid::new_v4())),
    );
    std::fs::create_dir_all(&directory.0).unwrap();
    let path = directory.0.join("sage.db");
    drop(open_or_migrate(&path).unwrap());
    let newer = MAX_SCHEMA_VERSION as i64 + 1;
    {
        let connection = Connection::open(&path).unwrap();
        connection
            .execute(
                "INSERT INTO schema_migrations(version, applied_at) VALUES (?1, 0)",
                [newer],
            )
            .unwrap();
        connection
            .pragma_update(None, "user_version", newer)
            .unwrap();
    }
    // A replacement left mid-flight by the newer Sage: recovery would
    // resume it if the refusal came any later.
    let paths = MigrationPaths::new(&path).unwrap();
    std::fs::write(&paths.replacement_build_intent, b"{\"newer\":true}").unwrap();
    if with_wal {
        // A checkpointed newer marker is conclusive even if the WAL is corrupt.
        std::fs::write(&paths.live_wal, [0x37_u8; 64]).unwrap();
    }
    let before = snapshot(&directory.0);

    let error = match open_or_migrate(&path) {
        Ok(_) => panic!("a newer profile must not open"),
        Err(error) => error,
    };

    assert!(
        matches!(
            error,
            AppError::NewerProfile { stored_version, supported_version }
                if stored_version == newer
                    && supported_version as usize == MAX_SCHEMA_VERSION
        ),
        "{error}"
    );
    assert!(error.to_string().contains("newer version of Sage"));
    assert_eq!(snapshot(&directory.0), before);
}

#[test]
fn oversized_live_files_are_refused_before_history_or_recovery() {
    use crate::acp::migration::limits::{LIVE_SQLITE_BYTES, SIDE_FILES_BYTES, WAL_HARD_BYTES};

    for (name, cap) in [
        ("sage.db", LIVE_SQLITE_BYTES),
        ("sage.db-wal", WAL_HARD_BYTES),
        ("sage.db-shm", SIDE_FILES_BYTES),
    ] {
        let directory = test_directory();
        let path = directory.0.join("sage.db");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch("CREATE TABLE schema_migrations(version INTEGER);")
            .unwrap();
        connection
            .execute(
                "INSERT INTO schema_migrations VALUES (?1)",
                [MAX_SCHEMA_VERSION as i64 + 1],
            )
            .unwrap();
        connection.close().unwrap();
        std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(directory.0.join(name))
            .unwrap()
            .set_len(cap + 1)
            .unwrap();
        let paths = MigrationPaths::new(&path).unwrap();
        std::fs::write(&paths.replacement_build_intent, b"{\"newer\":true}").unwrap();

        let error = match open_or_migrate(&path) {
            Ok(_) => panic!("oversized live files must not open"),
            Err(error) => error,
        };
        assert!(matches!(error, AppError::ResourceLimit(_)), "{error}");
        assert_eq!(
            std::fs::read(&paths.replacement_build_intent).unwrap(),
            b"{\"newer\":true}"
        );
    }
}

#[test]
fn an_unindexed_history_is_refused_before_recovery() {
    inspect_large_history(false);
}

#[test]
fn an_indexed_newer_history_is_refused_by_its_latest_version() {
    inspect_large_history(true);
}

#[test]
fn an_index_with_different_ordering_does_not_admit_a_history_scan() {
    let directory = test_directory();
    let path = directory.0.join("sage.db");
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE schema_migrations(
               version INTEGER COLLATE BINARY,
               PRIMARY KEY(version COLLATE NOCASE)
             ) WITHOUT ROWID;
             INSERT INTO schema_migrations VALUES (50000);",
        )
        .unwrap();
    connection.close().unwrap();
    let error = match open_or_migrate(&path) {
        Ok(_) => panic!("a history with an unproven ordering must not open"),
        Err(error) => error,
    };
    assert!(matches!(error, AppError::ResourceLimit(_)), "{error}");
}

fn inspect_large_history(indexed: bool) {
    let directory = test_directory();
    let path = directory.0.join("sage.db");
    let connection = Connection::open(&path).unwrap();
    let schema = if indexed {
        "CREATE TABLE schema_migrations(version INTEGER PRIMARY KEY)"
    } else {
        "CREATE TABLE schema_migrations(version INTEGER)"
    };
    connection.execute_batch(schema).unwrap();
    connection
        .execute_batch(
            "WITH RECURSIVE history(version) AS (
           VALUES(1) UNION ALL SELECT version+1 FROM history WHERE version < 50000
         ) INSERT INTO schema_migrations SELECT version FROM history;",
        )
        .unwrap();
    connection.close().unwrap();
    let paths = MigrationPaths::new(&path).unwrap();
    std::fs::write(&paths.replacement_build_intent, b"{\"newer\":true}").unwrap();
    let before = snapshot(&directory.0);

    let error = match open_or_migrate(&path) {
        Ok(_) => panic!("unsupported history must not open"),
        Err(error) => error,
    };
    if indexed {
        assert!(
            matches!(
                error,
                AppError::NewerProfile {
                    stored_version: 50_000,
                    ..
                }
            ),
            "{error}"
        );
    } else {
        assert!(matches!(error, AppError::ResourceLimit(_)), "{error}");
    }
    assert_eq!(snapshot(&directory.0), before);
}

#[test]
fn wal_only_newer_migrations_are_refused_before_pending_intent_recovery() {
    check_wal_preflight("none");
}

#[test]
fn an_unmanifested_probe_cannot_mask_a_wal_only_newer_profile() {
    check_wal_preflight("unmanifested");
}

#[test]
fn wal_only_newer_profiles_preserve_valid_interrupted_probes() {
    for prefix in ["manifest", "partial-main", "partial-wal"] {
        check_wal_preflight(prefix);
    }
}

fn check_wal_preflight(probe_prefix: &str) {
    // Exercise both markers together and history alone, with no live SHM.
    for update_marker in [true, false] {
        let source = test_directory();
        let path = source.0.join("sage.db");
        drop(open_or_migrate(&path).unwrap());
        let writer = Connection::open(&path).unwrap();
        writer
            .execute_batch(
                "PRAGMA journal_mode=WAL;
                 PRAGMA wal_autocheckpoint=0;
                 PRAGMA wal_checkpoint(TRUNCATE);",
            )
            .unwrap();
        writer
            .execute_batch("BEGIN IMMEDIATE; CREATE TABLE future_schema(value INTEGER);")
            .unwrap();
        let newer = MAX_SCHEMA_VERSION as i64 + 1;
        writer
            .execute(
                "INSERT INTO schema_migrations(version, applied_at) VALUES (?1, 0)",
                [newer],
            )
            .unwrap();
        if update_marker {
            writer.pragma_update(None, "user_version", newer).unwrap();
        }
        writer.execute_batch("COMMIT;").unwrap();
        assert_eq!(
            header_user_version(&path).unwrap(),
            Some(MAX_SCHEMA_VERSION as i32)
        );

        // Copy while the writer is alive: closing it would checkpoint the frames.
        let crashed = test_directory();
        let crashed_path = crashed.0.join("sage.db");
        let paths = MigrationPaths::new(&crashed_path).unwrap();
        std::fs::copy(&path, &paths.live).unwrap();
        std::fs::copy(source.0.join("sage.db-wal"), &paths.live_wal).unwrap();
        std::fs::write(&paths.replacement_build_intent, b"{\"newer\":true}").unwrap();
        if probe_prefix == "unmanifested" {
            std::fs::write(&paths.probe_main, b"foreign-probe").unwrap();
        } else if probe_prefix != "none" {
            probe::write_current_identity(&paths).unwrap();
            if probe_prefix != "manifest" {
                std::fs::copy(&paths.live, &paths.probe_main).unwrap();
                let partial = if probe_prefix == "partial-main" {
                    &paths.probe_main
                } else {
                    std::fs::copy(&paths.live_wal, &paths.probe_wal).unwrap();
                    &paths.probe_wal
                };
                let length = std::fs::metadata(partial).unwrap().len();
                std::fs::OpenOptions::new()
                    .write(true)
                    .open(partial)
                    .unwrap()
                    .set_len(length / 2)
                    .unwrap();
            }
        }
        let before = snapshot(&crashed.0);

        let error = match open_or_migrate(&crashed_path) {
            Ok(_) => panic!("a WAL-only newer profile must not open"),
            Err(error) => error,
        };
        assert!(
            matches!(error, AppError::NewerProfile { stored_version, .. } if stored_version == newer),
            "{error}"
        );
        assert_eq!(snapshot(&crashed.0), before);
        drop(writer);
    }
}

#[test]
fn wal_startup_reuses_one_bounded_snapshot_and_reads_committed_data() {
    use crate::acp::migration::test_failpoint::observe_snapshot_io;

    let source = test_directory();
    let path = source.0.join("sage.db");
    drop(open_or_migrate(&path).unwrap());
    let writer = Connection::open(&path).unwrap();
    writer
        .execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA wal_autocheckpoint=0;
             PRAGMA wal_checkpoint(TRUNCATE);
             UPDATE schema_migrations SET applied_at=4242 WHERE version=1;",
        )
        .unwrap();
    let crashed = test_directory();
    let paths = MigrationPaths::new(&crashed.0.join("sage.db")).unwrap();
    let main_bytes = std::fs::copy(&path, &paths.live).unwrap();
    let wal_bytes = std::fs::copy(source.0.join("sage.db-wal"), &paths.live_wal).unwrap();
    assert!(wal_bytes > 0);

    let io = observe_snapshot_io();
    let opened = open_or_migrate(&paths.live).unwrap();
    assert_eq!(io.copies(), (2, main_bytes + wal_bytes));
    // Three source hashes (capture, copy verification, pre-open check), and
    // one snapshot hash. Admission must not add a separate snapshot pass.
    assert_eq!(io.main_hashes(), 4);
    assert_eq!(
        opened
            .query_row::<i64, _, _>(
                "SELECT applied_at FROM schema_migrations WHERE version=1",
                [],
                |row| row.get(0),
            )
            .unwrap(),
        4242
    );
}

fn test_directory() -> TestDir {
    let directory = TestDir(
        crate::test_paths::temp_dir().join(format!("sage-preflight-{}", uuid::Uuid::new_v4())),
    );
    std::fs::create_dir_all(&directory.0).unwrap();
    // Startup always owns this lock; create it before byte-for-byte snapshots.
    drop(crate::acp::migration::platform::MigrationLock::acquire(&directory.0).unwrap());
    directory
}
