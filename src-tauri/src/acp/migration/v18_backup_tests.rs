use super::*;

fn populated_v16() -> TestDir {
    let directory = TestDir::new();
    create_v4(&directory.database(), "v16 backup recovery", true);
    let mut source = Connection::open(directory.database()).unwrap();
    schema_upgrade::apply_through_v16(&mut source).unwrap();
    assert_eq!(
        fingerprint::classify_connection(&source).unwrap(),
        SchemaClass::CurrentV16
    );
    source.close().unwrap();
    directory
}

fn assert_no_partial_backup(paths: &MigrationPaths) {
    for path in [
        &paths.backup_stage,
        &paths.backup_journal,
        &paths.backup_build_intent,
        &paths.backup_stage_owner,
        &paths.manifest_stage,
    ] {
        assert!(!path.exists(), "{}", path.display());
    }
}

fn assert_no_backup(paths: &MigrationPaths) {
    assert_no_partial_backup(paths);
    assert!(!paths.backup_final.exists());
    assert!(!paths.manifest_final.exists());
}

fn stage_backup_crash_prefix(paths: &MigrationPaths, prefix: &str) {
    let complete = matches!(
        prefix,
        "staged-pair" | "published-backup" | "published-pair"
    );
    if complete {
        ensure(paths).unwrap();
    }
    let source = Connection::open(&paths.live).unwrap();
    source.execute_batch("BEGIN IMMEDIATE").unwrap();
    let source_set = backup::source_set(paths, &source).unwrap();
    let intent = backup::begin_backup_build(paths, &source_set).unwrap();
    if prefix != "intent" {
        let mut anchor = stage_owner::create_bound_stage(
            &paths.backup_stage,
            &paths.backup_journal,
            &paths.backup_stage_owner,
            &intent.nonce,
        )
        .unwrap();
        if complete {
            fs::write(&paths.backup_stage, fs::read(&paths.backup_final).unwrap()).unwrap();
            platform::remove_synced(&paths.backup_final).unwrap();
            platform::rename_synced(&paths.manifest_final, &paths.manifest_stage).unwrap();
        } else {
            fs::write(&paths.backup_stage, b"owned-partial-backup").unwrap();
            if prefix == "partial-journal" {
                fs::write(&paths.backup_journal, b"owned-partial-journal").unwrap();
            }
        }
        anchor.verify_path(&paths.backup_stage).unwrap();
        anchor.sync().unwrap();
        if matches!(prefix, "published-backup" | "published-pair") {
            anchor
                .retire_journal_if_present(&paths.backup_journal)
                .unwrap();
            drop(anchor);
            platform::rename_synced(&paths.backup_stage, &paths.backup_final).unwrap();
            if prefix == "published-pair" {
                platform::rename_synced(&paths.manifest_stage, &paths.manifest_final).unwrap();
            }
        }
    }
    source.execute_batch("ROLLBACK").unwrap();
    source.close().unwrap();
}

#[test]
fn v16_image_upgrade_recovers_owned_pre_v5_backup_crash_prefixes() {
    for prefix in [
        "intent",
        "partial-stage",
        "partial-journal",
        "staged-pair",
        "published-backup",
        "published-pair",
    ] {
        let directory = populated_v16();
        let paths = MigrationPaths::new(&directory.database()).unwrap();
        let original = database_fingerprint(&open_read_only(&paths.live).unwrap()).unwrap();
        stage_backup_crash_prefix(&paths, prefix);
        let upgraded = open_or_migrate(&directory.database())
            .unwrap_or_else(|error| panic!("{prefix}: {error}"));
        assert_eq!(
            fingerprint::classify_connection(&upgraded).unwrap(),
            SchemaClass::CurrentV19
        );
        let versioned = v18_backup::paths_for_v18(&paths);
        assert_no_partial_backup(&paths);
        assert_no_partial_backup(&versioned);
        let retained_paths = if paths.backup_final.exists() {
            &paths
        } else {
            &versioned
        };
        let retained = backup::load_retained_pair(retained_paths).unwrap();
        assert_eq!(retained.source.database, original, "{prefix}");
        assert_eq!(retained.backup_database, original, "{prefix}");
        drop(upgraded);
        assert_eq!(
            fingerprint::classify_connection(&open_or_migrate(&directory.database()).unwrap())
                .unwrap(),
            SchemaClass::CurrentV19
        );
    }
}

#[test]
fn v16_image_upgrade_reuses_matching_pre_v5_backup_without_a_second_copy() {
    let directory = populated_v16();
    let paths = MigrationPaths::new(&directory.database()).unwrap();
    let original = ensure(&paths).unwrap();
    let backup_file = file_fingerprint(&paths.backup_final).unwrap();
    let manifest = fs::read(&paths.manifest_final).unwrap();
    let versioned = v18_backup::paths_for_v18(&paths);
    test_failpoint::arm("v17-to-v18-after-schema");
    assert!(open_or_migrate(&directory.database()).is_err());
    assert!(!test_failpoint::is_armed());
    assert_eq!(
        database_fingerprint(&open_read_only(&paths.live).unwrap()).unwrap(),
        original.source.database
    );
    assert_no_backup(&versioned);
    let upgraded = open_or_migrate(&directory.database()).unwrap();
    assert_eq!(
        fingerprint::classify_connection(&upgraded).unwrap(),
        SchemaClass::CurrentV19
    );
    assert_eq!(file_fingerprint(&paths.backup_final).unwrap(), backup_file);
    assert_eq!(fs::read(&paths.manifest_final).unwrap(), manifest);
    assert_eq!(
        backup::load_retained_pair(&paths).unwrap().source.database,
        original.source.database
    );
    assert_no_backup(&versioned);
    drop(upgraded);
    drop(open_or_migrate(&directory.database()).unwrap());
    assert_no_backup(&versioned);
    assert_eq!(file_fingerprint(&paths.backup_final).unwrap(), backup_file);
    assert_eq!(fs::read(&paths.manifest_final).unwrap(), manifest);
}

#[test]
fn v16_image_upgrade_preserves_unowned_pre_v5_backup_crash_artifacts() {
    let directory = populated_v16();
    let paths = MigrationPaths::new(&directory.database()).unwrap();
    let original = database_fingerprint(&open_read_only(&paths.live).unwrap()).unwrap();
    fs::write(&paths.backup_stage, b"unowned stage").unwrap();
    fs::write(&paths.backup_journal, b"unowned journal").unwrap();
    assert!(open_or_migrate(&directory.database()).is_err());
    assert_eq!(fs::read(&paths.backup_stage).unwrap(), b"unowned stage");
    assert_eq!(fs::read(&paths.backup_journal).unwrap(), b"unowned journal");
    assert_eq!(
        database_fingerprint(&open_read_only(&paths.live).unwrap()).unwrap(),
        original
    );
    assert_no_backup(&v18_backup::paths_for_v18(&paths));
}

#[test]
fn v16_image_upgrade_recovers_versioned_prefix_with_matching_pre_v5_backup() {
    let directory = populated_v16();
    let paths = MigrationPaths::new(&directory.database()).unwrap();
    let original = ensure(&paths).unwrap();
    let backup_file = file_fingerprint(&paths.backup_final).unwrap();
    let manifest = fs::read(&paths.manifest_final).unwrap();
    let versioned = v18_backup::paths_for_v18(&paths);
    stage_backup_crash_prefix(&versioned, "partial-stage");
    let upgraded = open_or_migrate(&directory.database()).unwrap();
    assert_eq!(
        fingerprint::classify_connection(&upgraded).unwrap(),
        SchemaClass::CurrentV19
    );
    assert_no_partial_backup(&versioned);
    assert_eq!(
        backup::load_retained_pair(&versioned)
            .unwrap()
            .source
            .database,
        original.source.database
    );
    assert_eq!(file_fingerprint(&paths.backup_final).unwrap(), backup_file);
    assert_eq!(fs::read(&paths.manifest_final).unwrap(), manifest);
    drop(upgraded);
    drop(open_or_migrate(&directory.database()).unwrap());
}
