use super::*;

fn populated_v17() -> TestDir {
    let directory = TestDir::new();
    create_v4(&directory.database(), "Images", true);
    let mut source = Connection::open(directory.database()).unwrap();
    schema_upgrade::apply_through_v17(&mut source).unwrap();
    assert_eq!(
        fingerprint::classify_connection(&source).unwrap(),
        SchemaClass::CurrentV17
    );
    source.close().unwrap();
    directory
}

fn item_objects(connection: &Connection) -> Vec<(String, String)> {
    connection.prepare("SELECT name,sql FROM sqlite_schema WHERE tbl_name='items' AND type IN ('index','trigger') AND sql IS NOT NULL ORDER BY type,name")
        .unwrap().query_map([], |row| Ok((row.get(0)?, row.get(1)?))).unwrap()
        .collect::<Result<_, _>>().unwrap()
}

fn item_rows(connection: &Connection) -> Vec<(String, String, String, i64)> {
    connection
        .prepare("SELECT id,payload_json,payload_digest,payload_bytes FROM items ORDER BY id")
        .unwrap()
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn populated_v17_image_upgrade_preserves_rows_objects_and_original_backup() {
    let directory = populated_v17();
    let source = open_read_only(&directory.database()).unwrap();
    let before = database_fingerprint(&source).unwrap();
    let rows = item_rows(&source);
    assert!(!rows.is_empty());
    let objects = item_objects(&source);
    drop(source);
    let upgraded = open_or_migrate(&directory.database()).unwrap();
    assert_eq!(
        fingerprint::classify_connection(&upgraded).unwrap(),
        SchemaClass::CurrentV21
    );
    assert_eq!(item_rows(&upgraded), rows);
    let upgraded_objects = item_objects(&upgraded);
    for object in &objects {
        assert!(upgraded_objects.contains(object));
    }
    let additions: Vec<_> = upgraded_objects
        .iter()
        .filter(|object| !objects.contains(object))
        .map(|object| object.0.as_str())
        .collect();
    assert_eq!(additions, ["idx_handoff_history", "idx_handoff_text_role"]);
    let foreign_keys: bool = upgraded
        .pragma_query_value(None, "foreign_keys", |row| row.get(0))
        .unwrap();
    assert!(foreign_keys);
    assert!(
        upgraded
            .query_row("PRAGMA foreign_key_check", [], |_| Ok(true))
            .optional()
            .unwrap()
            .is_none()
    );
    let paths = v18_backup::paths_for_v18(&MigrationPaths::new(&directory.database()).unwrap());
    let backup = open_read_only(&paths.backup_final).unwrap();
    assert_eq!(database_fingerprint(&backup).unwrap(), before);
    let backup_file = file_fingerprint(&paths.backup_final).unwrap();
    drop(backup);
    drop(upgraded);
    let reopened = open_or_migrate(&directory.database()).unwrap();
    assert_eq!(item_rows(&reopened), rows);
    assert_eq!(file_fingerprint(&paths.backup_final).unwrap(), backup_file);
}

#[test]
fn v17_image_upgrade_rolls_back_rebuild_and_markers_and_restores_foreign_keys() {
    let directory = populated_v17();
    let mut connection = Connection::open(directory.database()).unwrap();
    connection
        .pragma_update(None, "foreign_keys", true)
        .unwrap();
    let before = database_fingerprint(&connection).unwrap();
    test_failpoint::arm("v17-to-v18-after-markers");
    assert!(schema_upgrade::apply_v18(&mut connection).is_err());
    assert!(!test_failpoint::is_armed());
    assert_eq!(database_fingerprint(&connection).unwrap(), before);
    let foreign_keys: bool = connection
        .pragma_query_value(None, "foreign_keys", |row| row.get(0))
        .unwrap();
    assert!(foreign_keys);
    schema_upgrade::apply_v18(&mut connection).unwrap();
    assert_eq!(
        fingerprint::classify_connection(&connection).unwrap(),
        SchemaClass::CurrentV18
    );
}

#[test]
fn v18_image_schema_cannot_spoof_exact_v17_markers() {
    let mut connection = Connection::open_in_memory().unwrap();
    schema_upgrade::build_fresh_v18(&mut connection).unwrap();
    let actual = fingerprint::database_fingerprint(&connection).unwrap();
    connection
        .execute("DELETE FROM schema_migrations WHERE version=18", [])
        .unwrap();
    connection.pragma_update(None, "user_version", 17).unwrap();
    assert!(fingerprint::classify_connection(&connection).is_err());
    connection
        .execute("INSERT INTO schema_migrations VALUES(18,0)", [])
        .unwrap();
    connection.pragma_update(None, "user_version", 18).unwrap();
    assert_eq!(
        fingerprint::database_fingerprint(&connection)
            .unwrap()
            .schema_sha256,
        actual.schema_sha256
    );
    connection
        .execute_batch("DROP TRIGGER deny_terminal_item_mutation")
        .unwrap();
    assert!(fingerprint::classify_connection(&connection).is_err());
}

#[test]
fn v17_image_upgrade_retains_earlier_backup_and_pins_a_new_original_schema_copy() {
    let directory = TestDir::new();
    create_v4(&directory.database(), "Earlier backup", true);
    let paths = MigrationPaths::new(&directory.database()).unwrap();
    let earlier = ensure(&paths).unwrap();
    let earlier_file = file_fingerprint(&paths.backup_final).unwrap();
    let mut source = Connection::open(directory.database()).unwrap();
    schema_upgrade::apply_through_v17(&mut source).unwrap();
    source
        .execute("UPDATE chats SET title='Later work'", [])
        .unwrap();
    let original = database_fingerprint(&source).unwrap();
    assert_ne!(original, earlier.source.database);
    source.close().unwrap();
    let upgraded = open_or_migrate(&directory.database()).unwrap();
    assert_eq!(
        fingerprint::classify_connection(&upgraded).unwrap(),
        SchemaClass::CurrentV21
    );
    assert_eq!(file_fingerprint(&paths.backup_final).unwrap(), earlier_file);
    assert_eq!(
        backup::load_retained_pair(&paths).unwrap().source.database,
        earlier.source.database
    );
    let image_paths = v18_backup::paths_for_v18(&paths);
    assert_eq!(
        backup::load_retained_pair(&image_paths)
            .unwrap()
            .source
            .database,
        original
    );
    drop(upgraded);
    drop(open_or_migrate(&directory.database()).unwrap());
    assert_eq!(file_fingerprint(&paths.backup_final).unwrap(), earlier_file);
}

#[test]
fn v18_reopen_verifies_versioned_retained_backup_once() {
    let directory = populated_v17();
    drop(open_or_migrate(&directory.database()).unwrap());
    let loads = test_failpoint::observe_retained_pair_loads();
    let reopened = open_or_migrate(&directory.database()).unwrap();
    assert_eq!(loads.count(), 1);
    assert_eq!(
        fingerprint::classify_connection(&reopened).unwrap(),
        SchemaClass::CurrentV21
    );
}

#[test]
fn v16_image_upgrade_retains_earlier_backup_and_pins_a_new_original_schema_copy() {
    let directory = TestDir::new();
    create_v4(&directory.database(), "Earlier v16 backup", true);
    let paths = MigrationPaths::new(&directory.database()).unwrap();
    let earlier = ensure(&paths).unwrap();
    let earlier_file = file_fingerprint(&paths.backup_final).unwrap();
    let earlier_manifest = fs::read(&paths.manifest_final).unwrap();
    let mut source = Connection::open(directory.database()).unwrap();
    schema_upgrade::apply_through_v16(&mut source).unwrap();
    source
        .execute("UPDATE chats SET title='Later v16 work'", [])
        .unwrap();
    let original = database_fingerprint(&source).unwrap();
    assert_eq!(original.schema, SchemaClass::CurrentV16);
    assert_eq!(earlier.source.database.schema, SchemaClass::LegacyV4);
    let rows = item_rows(&source);
    let objects = item_objects(&source);
    assert!(!rows.is_empty());
    source.close().unwrap();
    let upgraded = open_or_migrate(&directory.database()).unwrap();
    assert_eq!(
        fingerprint::classify_connection(&upgraded).unwrap(),
        SchemaClass::CurrentV21
    );
    assert_eq!(item_rows(&upgraded), rows);
    let upgraded_objects = item_objects(&upgraded);
    for object in &objects {
        assert!(upgraded_objects.contains(object));
    }
    let additions: Vec<_> = upgraded_objects
        .iter()
        .filter(|object| !objects.contains(object))
        .map(|object| object.0.as_str())
        .collect();
    assert_eq!(additions, ["idx_handoff_history", "idx_handoff_text_role"]);
    assert_eq!(file_fingerprint(&paths.backup_final).unwrap(), earlier_file);
    assert_eq!(fs::read(&paths.manifest_final).unwrap(), earlier_manifest);
    assert_eq!(
        backup::load_retained_pair(&paths).unwrap().source.database,
        earlier.source.database
    );
    let versioned = v18_backup::paths_for_v18(&paths);
    let original_backup = backup::load_retained_pair(&versioned).unwrap();
    assert_eq!(original_backup.source.database, original);
    assert_eq!(original_backup.backup_database, original);
    let versioned_file = file_fingerprint(&versioned.backup_final).unwrap();
    drop(upgraded);
    let reopened = open_or_migrate(&directory.database()).unwrap();
    assert_eq!(item_rows(&reopened), rows);
    assert_eq!(
        file_fingerprint(&versioned.backup_final).unwrap(),
        versioned_file
    );
    assert_eq!(file_fingerprint(&paths.backup_final).unwrap(), earlier_file);
    assert_eq!(fs::read(&paths.manifest_final).unwrap(), earlier_manifest);
}

#[test]
fn v17_image_upgrade_rolls_back_rebuild_before_markers_and_restores_foreign_keys() {
    let directory = populated_v17();
    let mut connection = Connection::open(directory.database()).unwrap();
    connection
        .pragma_update(None, "foreign_keys", true)
        .unwrap();
    let before = database_fingerprint(&connection).unwrap();
    let rows = item_rows(&connection);
    let objects = item_objects(&connection);
    test_failpoint::arm("v17-to-v18-after-schema");
    assert!(schema_upgrade::apply_v18(&mut connection).is_err());
    assert!(!test_failpoint::is_armed());
    assert_eq!(database_fingerprint(&connection).unwrap(), before);
    assert_eq!(item_rows(&connection), rows);
    assert_eq!(item_objects(&connection), objects);
    let marker: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    let migration: i64 = connection
        .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!((marker, migration), (17, 17));
    let foreign_keys: bool = connection
        .pragma_query_value(None, "foreign_keys", |row| row.get(0))
        .unwrap();
    assert!(foreign_keys);
    schema_upgrade::apply_v18(&mut connection).unwrap();
    assert_eq!(
        fingerprint::classify_connection(&connection).unwrap(),
        SchemaClass::CurrentV18
    );
}

#[test]
fn v16_image_upgrade_retries_after_schema_failure_with_original_backup_intact() {
    let directory = TestDir::new();
    create_v4(&directory.database(), "Earlier v16 retry backup", true);
    let paths = MigrationPaths::new(&directory.database()).unwrap();
    ensure(&paths).unwrap();
    let earlier_file = file_fingerprint(&paths.backup_final).unwrap();
    let earlier_manifest = fs::read(&paths.manifest_final).unwrap();
    let mut source = Connection::open(directory.database()).unwrap();
    schema_upgrade::apply_through_v16(&mut source).unwrap();
    let original = database_fingerprint(&source).unwrap();
    source.close().unwrap();
    test_failpoint::arm("v17-to-v18-after-schema");
    assert!(open_or_migrate(&directory.database()).is_err());
    assert!(!test_failpoint::is_armed());
    let source = open_read_only(&directory.database()).unwrap();
    assert_eq!(database_fingerprint(&source).unwrap(), original);
    drop(source);
    let versioned = v18_backup::paths_for_v18(&paths);
    assert_eq!(
        backup::load_retained_pair(&versioned)
            .unwrap()
            .source
            .database,
        original
    );
    let versioned_file = file_fingerprint(&versioned.backup_final).unwrap();
    let retried = open_or_migrate(&directory.database()).unwrap();
    assert_eq!(
        fingerprint::classify_connection(&retried).unwrap(),
        SchemaClass::CurrentV21
    );
    assert_eq!(
        file_fingerprint(&versioned.backup_final).unwrap(),
        versioned_file
    );
    assert_eq!(file_fingerprint(&paths.backup_final).unwrap(), earlier_file);
    assert_eq!(fs::read(&paths.manifest_final).unwrap(), earlier_manifest);
}

#[test]
fn v17_image_upgrade_retries_after_marker_failure_with_original_backup_intact() {
    let directory = populated_v17();
    let source = open_read_only(&directory.database()).unwrap();
    let original = database_fingerprint(&source).unwrap();
    drop(source);
    test_failpoint::arm("v17-to-v18-after-markers");
    assert!(open_or_migrate(&directory.database()).is_err());
    assert!(!test_failpoint::is_armed());
    let source = open_read_only(&directory.database()).unwrap();
    assert_eq!(database_fingerprint(&source).unwrap(), original);
    drop(source);
    let paths = v18_backup::paths_for_v18(&MigrationPaths::new(&directory.database()).unwrap());
    assert_eq!(
        backup::load_retained_pair(&paths).unwrap().source.database,
        original
    );
    let backup_file = file_fingerprint(&paths.backup_final).unwrap();
    let upgraded = open_or_migrate(&directory.database()).unwrap();
    assert_eq!(
        fingerprint::classify_connection(&upgraded).unwrap(),
        SchemaClass::CurrentV21
    );
    assert_eq!(file_fingerprint(&paths.backup_final).unwrap(), backup_file);
}

#[test]
fn v18_reopen_rejects_incomplete_image_backup_without_mutating_live_database() {
    let directory = populated_v17();
    drop(open_or_migrate(&directory.database()).unwrap());
    let before = file_fingerprint(&directory.database()).unwrap();
    let paths = v18_backup::paths_for_v18(&MigrationPaths::new(&directory.database()).unwrap());
    fs::remove_file(&paths.manifest_final).unwrap();
    assert!(open_or_migrate(&directory.database()).is_err());
    assert_eq!(file_fingerprint(&directory.database()).unwrap(), before);
    assert!(paths.backup_final.exists());
}

#[test]
fn v18_backup_fingerprints_admit_only_exact_versioned_artifact_names() {
    let directory = TestDir::new();
    let paths = v18_backup::paths_for_v18(&MigrationPaths::new(&directory.database()).unwrap());
    for path in [
        &paths.backup_stage,
        &paths.backup_final,
        &paths.backup_journal,
    ] {
        fs::write(path, b"bounded artifact").unwrap();
        let fingerprint = file_fingerprint(path).unwrap();
        assert!(fingerprint.present);
        assert_eq!(fingerprint.bytes, 16);
    }
    let unexpected = directory.0.join("sage.pre-v18-other.db");
    fs::write(&unexpected, b"unrecognized").unwrap();
    assert!(file_fingerprint(&unexpected).is_err());
    assert!(unexpected.exists());
}

#[cfg(windows)]
#[test]
fn legacy_replacement_rejects_v18_backup_artifacts_before_publication() {
    let directory = TestDir::new();
    create_v4(&directory.database(), "Legacy replacement", true);
    let paths = MigrationPaths::new(&directory.database()).unwrap();
    prepare_replacement(&paths);
    let source = file_fingerprint(&paths.live).unwrap();
    let replacement = file_fingerprint(&paths.replacement_stage).unwrap();
    let manifest = fs::read(&paths.replacement_manifest).unwrap();
    let image_paths = v18_backup::paths_for_v18(&paths);
    fs::write(&image_paths.backup_stage, b"stray image stage").unwrap();
    assert!(open_or_migrate(&directory.database()).is_err());
    assert_eq!(file_fingerprint(&paths.live).unwrap(), source);
    assert_eq!(
        file_fingerprint(&paths.replacement_stage).unwrap(),
        replacement
    );
    assert_eq!(fs::read(&paths.replacement_manifest).unwrap(), manifest);
    assert!(image_paths.backup_stage.exists());
}
