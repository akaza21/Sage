use std::{fs, path::Path, time::Duration};

use rusqlite::{Connection, OpenFlags, backup::Backup};

use crate::{
    error::{AppError, AppResult},
    persistence::now_millis,
};

use super::{
    fingerprint::{
        DatabaseFingerprint, classify_connection, database_fingerprint, file_fingerprint,
        open_read_only,
    },
    limits::{MIGRATION_FREE_BYTES, TEMP_SQLITE_BYTES, preflight_original_schema},
    manifest::{BackupBuildIntent, BackupManifest, BackupSource, read, write_stage},
    paths::MigrationPaths,
    platform::{
        anchor_existing_sqlite, remove_synced, rename_synced, require_free_bytes, sync_file,
    },
    stage_owner,
};

pub(super) struct VerifiedBackup {
    pub(super) source: BackupSource,
    pub(super) backup_file: super::fingerprint::FileFingerprint,
    pub(super) backup_database: DatabaseFingerprint,
}

pub(super) fn ensure(paths: &MigrationPaths) -> AppResult<VerifiedBackup> {
    require_free_bytes(paths.parent(), MIGRATION_FREE_BYTES)?;
    let live_anchor = anchor_existing_sqlite(&paths.live, true)?;
    let source = Connection::open_with_flags(
        &paths.live,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    source.execute_batch("PRAGMA busy_timeout=0; BEGIN IMMEDIATE;")?;
    let class = classify_connection(&source)?;
    if !class.requires_retained_backup() {
        return invalid("backup source does not require an original-schema backup");
    }
    preflight_original_schema(&source, &paths.live, class)?;
    verify_sqlite(&source)?;
    let source_set = source_set(paths, &source)?;

    recover_backup_publication(paths, &source_set)?;
    if paths.backup_final.exists() && paths.manifest_final.exists() {
        let verified = verify_final_pair(paths, &source_set)?;
        cleanup_backup_build(paths)?;
        source.execute_batch("ROLLBACK")?;
        live_anchor.verify_path(&paths.live)?;
        source.close().map_err(|(_, error)| error)?;
        return Ok(verified);
    }

    let build_intent = begin_backup_build(paths, &source_set)?;
    create_backup_stage(paths, &build_intent.nonce)?;
    let backup_database = verify_backup_database(&paths.backup_stage, &source_set.database)?;
    let backup_file = file_fingerprint(&paths.backup_stage)?;
    let manifest = BackupManifest::new(
        source_set.clone(),
        backup_file.clone(),
        backup_database.clone(),
        now_millis(),
    );
    write_stage(&paths.manifest_stage, &manifest)?;
    verify_stage_pair(paths, &source_set)?;
    publish_stages(paths, &build_intent.nonce)?;
    let verified = verify_final_pair(paths, &source_set)?;
    cleanup_backup_build(paths)?;
    source.execute_batch("ROLLBACK")?;
    live_anchor.verify_path(&paths.live)?;
    source.close().map_err(|(_, error)| error)?;
    Ok(verified)
}

pub(super) fn verify_retained_pair(paths: &MigrationPaths) -> AppResult<()> {
    load_retained_pair(paths).map(|_| ())
}

pub(super) fn load_retained_pair(paths: &MigrationPaths) -> AppResult<VerifiedBackup> {
    let manifest: BackupManifest = read(&paths.manifest_final)?;
    manifest.validate_identity()?;
    if !manifest.source.database.schema.requires_retained_backup()
        || manifest.backup_database != manifest.source.database
    {
        return invalid("retained backup manifest has an invalid legacy source binding");
    }
    if file_fingerprint(&paths.backup_final)? != manifest.backup {
        return invalid("retained backup bytes do not match its manifest");
    }
    let actual = verify_backup_database(&paths.backup_final, &manifest.source.database)?;
    if actual != manifest.backup_database {
        return invalid("retained backup logical fingerprint does not match its manifest");
    }
    Ok(VerifiedBackup {
        source: manifest.source,
        backup_file: manifest.backup,
        backup_database: manifest.backup_database,
    })
}

fn recover_backup_publication(paths: &MigrationPaths, source: &BackupSource) -> AppResult<()> {
    let state = (
        paths.backup_stage.exists(),
        paths.manifest_stage.exists(),
        paths.backup_final.exists(),
        paths.manifest_final.exists(),
    );
    match state {
        (false, false, false, false) | (false, false, true, true) => Ok(()),
        (true, false, false, false) => {
            let intent = read_backup_intent(paths, source)?;
            stage_owner::remove_owned_partial(
                &paths.backup_stage,
                &paths.backup_journal,
                &paths.backup_stage_owner,
                &intent.nonce,
            )
        }
        (true, true, false, false) => {
            let intent = read_backup_intent(paths, source)?;
            let anchor = stage_owner::verify_bound_stage(
                &paths.backup_stage,
                &paths.backup_journal,
                &paths.backup_stage_owner,
                &intent.nonce,
                false,
            )?;
            verify_stage_pair(paths, source)?;
            anchor.verify_path(&paths.backup_stage)?;
            drop(anchor);
            publish_stages(paths, &intent.nonce)
        }
        (false, true, true, false) => {
            let intent = read_backup_intent(paths, source)?;
            stage_owner::retire_orphan_owner(
                &paths.backup_stage,
                &paths.backup_journal,
                &paths.backup_stage_owner,
                &intent.nonce,
            )?;
            verify_manifest_and_backup(&paths.manifest_stage, &paths.backup_final, source)?;
            rename_synced(&paths.manifest_stage, &paths.manifest_final)
        }
        _ => invalid("unlisted or corrupt backup publication state"),
    }
}

fn create_backup_stage(paths: &MigrationPaths, nonce: &str) -> AppResult<()> {
    if paths.backup_stage.exists() {
        return invalid("backup stage unexpectedly exists");
    }
    let source_anchor = anchor_existing_sqlite(&paths.live, false)?;
    let source = Connection::open_with_flags(
        &paths.live,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let destination_anchor = stage_owner::create_bound_stage(
        &paths.backup_stage,
        &paths.backup_journal,
        &paths.backup_stage_owner,
        nonce,
    )?;
    let mut destination = Connection::open(&paths.backup_stage)?;
    destination.pragma_update(None, "journal_mode", "TRUNCATE")?;
    let page_size: i64 = source.pragma_query_value(None, "page_size", |row| row.get(0))?;
    destination.pragma_update(None, "page_size", page_size)?;
    destination.pragma_update(
        None,
        "max_page_count",
        (TEMP_SQLITE_BYTES as i64) / page_size.max(1),
    )?;
    Backup::new(&source, &mut destination)?.run_to_completion(256, Duration::ZERO, None)?;
    source.close().map_err(|(_, error)| error)?;
    destination.close().map_err(|(_, error)| error)?;
    source_anchor.verify_path(&paths.live)?;
    destination_anchor.verify_path(&paths.backup_stage)?;
    destination_anchor.sync()?;
    drop(destination_anchor);
    let bytes = fs::metadata(&paths.backup_stage)?.len();
    if bytes > TEMP_SQLITE_BYTES {
        return Err(AppError::ResourceLimit(
            "backup stage exceeds the 1 GiB migration limit".into(),
        ));
    }
    sync_file(&paths.backup_stage)?;
    Ok(())
}

pub(super) fn begin_backup_build(
    paths: &MigrationPaths,
    source: &BackupSource,
) -> AppResult<BackupBuildIntent> {
    if paths.backup_build_intent.exists() {
        let intent = read_backup_intent(paths, source)?;
        stage_owner::retire_orphan_owner(
            &paths.backup_stage,
            &paths.backup_journal,
            &paths.backup_stage_owner,
            &intent.nonce,
        )?;
        return Ok(intent);
    }
    if paths.backup_stage.exists()
        || paths.backup_journal.exists()
        || paths.backup_stage_owner.exists()
    {
        return invalid("unowned backup build artifacts are preserved");
    }
    let intent = BackupBuildIntent::new(
        source.database.clone(),
        file_name(&paths.backup_stage)?.into(),
        file_name(&paths.backup_journal)?.into(),
        uuid::Uuid::new_v4().to_string(),
    );
    write_stage(&paths.backup_build_intent, &intent)?;
    Ok(intent)
}

fn read_backup_intent(
    paths: &MigrationPaths,
    source: &BackupSource,
) -> AppResult<BackupBuildIntent> {
    let intent: BackupBuildIntent = read(&paths.backup_build_intent)?;
    intent.validate_identity()?;
    if intent.source_database != source.database
        || intent.backup_stage_name != file_name(&paths.backup_stage)?
        || intent.backup_journal_name != file_name(&paths.backup_journal)?
    {
        return invalid("backup build intent does not match the pinned source");
    }
    Ok(intent)
}

fn cleanup_backup_build(paths: &MigrationPaths) -> AppResult<()> {
    if paths.backup_journal.exists() || paths.backup_stage.exists() {
        return invalid("backup build artifacts remain after publication");
    }
    stage_owner::retire_owner(&paths.backup_stage_owner)?;
    remove_synced(&paths.backup_build_intent)
}

fn file_name(path: &Path) -> AppResult<&str> {
    path.file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| AppError::InvalidRequest("backup artifact filename is not UTF-8".into()))
}

fn verify_stage_pair(paths: &MigrationPaths, source: &BackupSource) -> AppResult<()> {
    verify_manifest_and_backup(&paths.manifest_stage, &paths.backup_stage, source)
}

fn publish_stages(paths: &MigrationPaths, nonce: &str) -> AppResult<()> {
    let mut anchors = stage_owner::verify_bound_stage(
        &paths.backup_stage,
        &paths.backup_journal,
        &paths.backup_stage_owner,
        nonce,
        false,
    )?;
    anchors.retire_journal_if_present(&paths.backup_journal)?;
    anchors.verify_path(&paths.backup_stage)?;
    drop(anchors);
    rename_synced(&paths.backup_stage, &paths.backup_final)?;
    rename_synced(&paths.manifest_stage, &paths.manifest_final)
}

fn verify_final_pair(paths: &MigrationPaths, source: &BackupSource) -> AppResult<VerifiedBackup> {
    let manifest: BackupManifest = read(&paths.manifest_final)?;
    manifest.validate_identity()?;
    if &manifest.source != source {
        return invalid("retained backup is bound to a different source database");
    }
    let backup_file = file_fingerprint(&paths.backup_final)?;
    if backup_file != manifest.backup {
        return invalid("retained backup bytes do not match its manifest");
    }
    let backup_database = verify_backup_database(&paths.backup_final, &source.database)?;
    if backup_database != manifest.backup_database {
        return invalid("retained backup logical fingerprint does not match its manifest");
    }
    Ok(VerifiedBackup {
        source: source.clone(),
        backup_file,
        backup_database,
    })
}

fn verify_manifest_and_backup(
    manifest_path: &Path,
    backup_path: &Path,
    source: &BackupSource,
) -> AppResult<()> {
    let manifest: BackupManifest = read(manifest_path)?;
    manifest.validate_identity()?;
    if &manifest.source != source {
        return invalid("backup publication source identity changed");
    }
    if file_fingerprint(backup_path)? != manifest.backup {
        return invalid("backup publication file digest mismatch");
    }
    let database = verify_backup_database(backup_path, &source.database)?;
    if database != manifest.backup_database {
        return invalid("backup publication logical fingerprint mismatch");
    }
    Ok(())
}

fn verify_backup_database(
    path: &Path,
    expected: &DatabaseFingerprint,
) -> AppResult<DatabaseFingerprint> {
    let connection = open_read_only(path)?;
    verify_sqlite(&connection)?;
    let actual = database_fingerprint(&connection)?;
    if &actual != expected {
        return invalid("backup is not a logical copy of the pinned source");
    }
    Ok(actual)
}

pub(super) fn source_set(
    paths: &MigrationPaths,
    connection: &Connection,
) -> AppResult<BackupSource> {
    let database = database_fingerprint(connection)?;
    if !database.schema.requires_retained_backup() {
        return invalid("source set does not require an original-schema backup");
    }
    Ok(BackupSource {
        database,
        main: file_fingerprint(&paths.live)?,
        wal: file_fingerprint(&paths.live_wal)?,
    })
}

pub(super) fn verify_sqlite(connection: &Connection) -> AppResult<()> {
    let quick: String = connection.query_row("PRAGMA quick_check(1)", [], |row| row.get(0))?;
    if quick != "ok" {
        return invalid("SQLite quick_check failed");
    }
    let foreign_key_error: bool = connection
        .query_row("PRAGMA foreign_key_check", [], |_| Ok(true))
        .optional()?
        .unwrap_or(false);
    if foreign_key_error {
        return invalid("SQLite foreign_key_check failed");
    }
    Ok(())
}

fn invalid<T>(message: &str) -> AppResult<T> {
    Err(AppError::InvalidRequest(message.into()))
}

use rusqlite::OptionalExtension;
