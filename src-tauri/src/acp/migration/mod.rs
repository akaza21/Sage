mod backup;
mod build_intent;
mod file_artifact;
mod fingerprint;
mod identity;
mod image_files;
mod legacy;
mod legacy_events;
mod legacy_project;
mod limits;
mod manifest;
mod paths;
mod platform;
mod platform_anchor;
#[cfg(test)]
mod portable_coverage_tests;
mod probe;
mod publish;
mod schema_upgrade;
mod stage_owner;
#[cfg(test)]
mod test_failpoint;
mod tool_result_fold;
#[cfg(test)]
mod tool_result_fold_tests;

#[allow(unused_imports)]
pub(crate) use identity::legacy_binding_digest;
pub(crate) use image_files::{read_image_file, remove_image_file};

use std::{fs, path::Path, time::Duration};

use rusqlite::{Connection, TransactionBehavior, backup::Backup};

use crate::{
    error::{AppError, AppResult},
    persistence::now_millis,
    project_migration,
};

use self::{
    backup::{VerifiedBackup, ensure, verify_retained_pair, verify_sqlite},
    fingerprint::{
        SchemaClass, classify_path, database_fingerprint, file_fingerprint, open_read_only,
    },
    limits::{LIVE_SQLITE_BYTES, TEMP_SQLITE_BYTES},
    paths::MigrationPaths,
    platform::{MigrationLock, sync_file, sync_parent},
    publish::recover_or_publish,
    schema_upgrade::{apply_through_v15, build_fresh_v15, upgrade_current_through_v9},
};

#[cfg(test)]
use self::schema_upgrade::{apply_v7, apply_v8, apply_v9, apply_v10, apply_v11, apply_v12};

#[cfg(all(test, windows))]
use self::manifest::{ReplacementManifest, write_stage};
#[cfg(test)]
use self::schema_upgrade::{apply_v5, apply_v6};

const INITIAL: &str = include_str!("../../../migrations/001_initial.sql");
const V2: &str = include_str!("../../../migrations/002_diff_tabs.sql");
const V3: &str = include_str!("../../../migrations/003_editor_buffers.sql");
const V6: &str = include_str!("../../../migrations/006_resource_trash_intents.sql");
const V7: &str = include_str!("../../../migrations/007_acp_activation_abort.sql");
const V8: &str = include_str!("../../../migrations/008_permission_decisions.sql");
const V9: &str = include_str!("../../../migrations/009_cancelled_permissions.sql");
const V10: &str = include_str!("../../../migrations/010_acp_file_change_snapshots.sql");
const V11: &str = include_str!("../../../migrations/011_turn_usage_snapshots.sql");
const V12: &str = include_str!("../../../migrations/012_browser_tabs.sql");
const V13: &str = include_str!("../../../migrations/013_chat_queued_prompts.sql");
const V14: &str = include_str!("../../../migrations/014_chat_image_artifacts.sql");
const V15: &str = include_str!("../../../migrations/015_acp_child_thread_bindings.sql");

pub(crate) struct OpenedDatabase {
    connection: Connection,
    live_path_anchor: LivePathAnchor,
}

#[cfg(all(test, not(windows)))]
pub(crate) fn complete_v4_upgrade_for_test(connection: &mut Connection) -> AppResult<()> {
    apply_through_v15(connection)
}

#[derive(Debug)]
pub(crate) struct LivePathAnchor {
    _anchor: Option<platform::SqlitePathAnchor>,
}

impl OpenedDatabase {
    pub(crate) fn into_parts(self) -> (Connection, LivePathAnchor) {
        (self.connection, self.live_path_anchor)
    }
}

impl std::ops::Deref for OpenedDatabase {
    type Target = Connection;

    fn deref(&self) -> &Self::Target {
        &self.connection
    }
}

impl std::ops::DerefMut for OpenedDatabase {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.connection
    }
}

pub(crate) fn open_or_migrate(path: &Path) -> AppResult<OpenedDatabase> {
    if path == Path::new(":memory:") {
        let mut connection = Connection::open_in_memory()?;
        build_fresh_v15(&mut connection)?;
        return Ok(OpenedDatabase {
            connection,
            live_path_anchor: LivePathAnchor { _anchor: None },
        });
    }
    let paths = MigrationPaths::new(path)?;
    fs::create_dir_all(paths.parent())?;
    let _lock = MigrationLock::acquire(paths.parent())?;
    paths.validate_containment()?;

    if paths.replacement_manifest.exists() {
        if paths.replacement_build_intent.exists() {
            build_intent::finalize_published_manifest(&paths)?;
        }
        let _ = recover_or_publish(&paths)?;
    } else if paths.replacement_build_intent.exists() {
        build_intent::resume(&paths)?;
        let _ = recover_or_publish(&paths)?;
    } else if paths.has_replacement_artifact() {
        recover_unmanifested_stage(&paths)?;
    }

    match probe::classify_live(&paths)? {
        SchemaClass::Fresh => {
            reject_unexpected_backup_artifacts(&paths)?;
            let anchor = if path.exists() {
                let anchor = platform::anchor_existing_sqlite(path, true)?;
                if fs::metadata(path)?.len() != 0 {
                    return invalid("fresh SQLite path changed before its anchored open");
                }
                anchor
            } else {
                platform::create_sqlite_stage(path)?
            };
            let mut connection = Connection::open(path)?;
            build_fresh_v15(&mut connection)?;
            verify_current(&connection)?;
            anchor.verify_path(path)?;
            Ok(opened_database(connection, anchor))
        }
        SchemaClass::CurrentV15 => {
            reject_partial_backup_publication(&paths)?;
            let anchor = platform::anchor_existing_sqlite(path, true)?;
            if probe::classify_live(&paths)? != SchemaClass::CurrentV15 {
                return invalid("current SQLite path changed before its anchored open");
            }
            let connection = Connection::open(path)?;
            verify_current(&connection)?;
            anchor.verify_path(path)?;
            Ok(opened_database(connection, anchor))
        }
        SchemaClass::CurrentV14 => upgrade_current_through_v9(&paths, SchemaClass::CurrentV14),
        SchemaClass::CurrentV13 => upgrade_current_through_v9(&paths, SchemaClass::CurrentV13),
        SchemaClass::CurrentV12 => upgrade_current_through_v9(&paths, SchemaClass::CurrentV12),
        SchemaClass::CurrentV11 => upgrade_current_through_v9(&paths, SchemaClass::CurrentV11),
        SchemaClass::CurrentV10 => upgrade_current_through_v9(&paths, SchemaClass::CurrentV10),
        SchemaClass::CurrentV10LegacySessionEvents => {
            upgrade_current_through_v9(&paths, SchemaClass::CurrentV10LegacySessionEvents)
        }
        SchemaClass::CurrentV9 => upgrade_current_through_v9(&paths, SchemaClass::CurrentV9),
        SchemaClass::CurrentV8 => upgrade_current_through_v9(&paths, SchemaClass::CurrentV8),
        SchemaClass::CurrentV7 => upgrade_current_through_v9(&paths, SchemaClass::CurrentV7),
        SchemaClass::CurrentV6 => upgrade_current_through_v9(&paths, SchemaClass::CurrentV6),
        SchemaClass::CurrentV5 => upgrade_current_through_v9(&paths, SchemaClass::CurrentV5),
        class if class.is_legacy() => migrate_replacement(&paths),
        _ => Err(AppError::InvalidRequest(
            "database schema is not admitted for migration".into(),
        )),
    }
}

fn migrate_replacement(paths: &MigrationPaths) -> AppResult<OpenedDatabase> {
    let verified_backup = ensure(paths)?;
    build_intent::begin(paths, &verified_backup)?;
    build_intent::resume(paths)?;
    if !recover_or_publish(paths)? {
        return Err(AppError::InvalidRequest(
            "replacement publication did not converge".into(),
        ));
    }
    #[cfg(test)]
    test_failpoint::hit("postcommit-reopen")?;
    let anchor = platform::anchor_existing_sqlite(&paths.live, true)?;
    let connection = Connection::open(&paths.live)?;
    verify_current(&connection)?;
    anchor.verify_path(&paths.live)?;
    Ok(opened_database(connection, anchor))
}

fn opened_database(connection: Connection, anchor: platform::SqlitePathAnchor) -> OpenedDatabase {
    OpenedDatabase {
        connection,
        live_path_anchor: LivePathAnchor {
            _anchor: Some(anchor),
        },
    }
}

fn build_replacement(
    paths: &MigrationPaths,
    backup: &VerifiedBackup,
    nonce: &str,
) -> AppResult<()> {
    if paths.replacement_stage.exists()
        || paths.replacement_journal.exists()
        || paths.replacement_stage_owner.exists()
    {
        return invalid("replacement build target already exists");
    }
    let source = open_read_only(&paths.backup_final)?;
    if database_fingerprint(&source)? != backup.backup_database {
        return invalid("verified backup changed before replacement build");
    }
    let anchor = stage_owner::create_bound_stage(
        &paths.replacement_stage,
        &paths.replacement_journal,
        &paths.replacement_stage_owner,
        nonce,
    )?;
    fail_replacement_build_if_injected()?;
    let mut destination = Connection::open(&paths.replacement_stage)?;
    destination.pragma_update(None, "journal_mode", "TRUNCATE")?;
    let page_size: i64 = destination.pragma_query_value(None, "page_size", |row| row.get(0))?;
    destination.pragma_update(
        None,
        "max_page_count",
        (TEMP_SQLITE_BYTES as i64) / page_size.max(1),
    )?;
    Backup::new(&source, &mut destination)?.run_to_completion(256, Duration::ZERO, None)?;
    drop(source);
    apply_through_v15(&mut destination)?;
    verify_current(&destination)?;
    destination.close().map_err(|(_, error)| error)?;
    anchor.verify_path(&paths.replacement_stage)?;
    anchor.sync()?;
    drop(anchor);
    if fs::metadata(&paths.replacement_stage)?.len() > LIVE_SQLITE_BYTES {
        return Err(AppError::ResourceLimit(
            "replacement database exceeds the 1 GiB limit".into(),
        ));
    }
    sync_file(&paths.replacement_stage)?;
    sync_parent(&paths.replacement_stage)
}

#[cfg(all(test, windows))]
fn inject_replacement_build_failure_once() {
    test_failpoint::arm("replacement-build");
}

fn fail_replacement_build_if_injected() -> AppResult<()> {
    #[cfg(test)]
    test_failpoint::hit("replacement-build")?;
    Ok(())
}

fn checkpoint_source(
    paths: &MigrationPaths,
    backup: &VerifiedBackup,
) -> AppResult<manifest::SourceSet> {
    let anchor = platform::anchor_existing_sqlite(&paths.live, true)?;
    let connection = Connection::open(&paths.live)?;
    connection.execute_batch("PRAGMA busy_timeout=0; BEGIN IMMEDIATE;")?;
    if database_fingerprint(&connection)? != backup.source.database {
        return invalid("legacy source changed after backup publication");
    }
    connection.execute_batch("ROLLBACK")?;
    let (busy, _log, _checkpointed): (i64, i64, i64) =
        connection.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?;
    if busy != 0 {
        return invalid("legacy WAL did not converge before replacement");
    }
    let database = database_fingerprint(&connection)?;
    if database != backup.source.database {
        return invalid("legacy logical fingerprint changed during checkpoint");
    }
    connection.close().map_err(|(_, error)| error)?;
    anchor.verify_path(&paths.live)?;
    let after = manifest::SourceSet {
        database,
        main: file_fingerprint(&paths.live)?,
        wal: file_fingerprint(&paths.live_wal)?,
        shm: file_fingerprint(&paths.live_shm)?,
    };
    if after.wal.present && after.wal.bytes != 0 {
        return invalid("legacy WAL remains nonempty after truncate checkpoint");
    }
    Ok(after)
}

fn verify_current(connection: &Connection) -> AppResult<()> {
    if fingerprint::classify_connection(connection)? != SchemaClass::CurrentV15 {
        return invalid("database did not reach exact schema v15");
    }
    verify_sqlite(connection)?;
    let store_rows: i64 = connection.query_row(
        "SELECT COUNT(*) FROM acp_store_usage WHERE singleton=1",
        [],
        |row| row.get(0),
    )?;
    if store_rows != 1 {
        return invalid("ACP global usage ledger is missing or duplicated");
    }
    Ok(())
}

fn recover_unmanifested_stage(paths: &MigrationPaths) -> AppResult<()> {
    let only_stage = paths.replacement_stage.exists()
        && !paths.retired_live.exists()
        && !paths.retired_wal.exists()
        && !paths.retired_shm.exists();
    if !only_stage || !classify_path(&paths.live)?.is_legacy() {
        return invalid("unmanifested replacement artifacts block migration");
    }
    let staged = open_read_only(&paths.replacement_stage)?;
    verify_current(&staged)?;
    drop(staged);
    invalid("unmanifested v5 stage cannot be source-bound and is preserved")
}

fn reject_unexpected_backup_artifacts(paths: &MigrationPaths) -> AppResult<()> {
    if paths.backup_stage.exists()
        || paths.backup_final.exists()
        || paths.backup_journal.exists()
        || paths.backup_build_intent.exists()
        || paths.backup_stage_owner.exists()
        || paths.manifest_stage.exists()
        || paths.manifest_final.exists()
    {
        return invalid("migration backup artifacts coexist with a fresh database");
    }
    Ok(())
}

fn reject_partial_backup_publication(paths: &MigrationPaths) -> AppResult<()> {
    if paths.backup_stage.exists()
        || paths.backup_journal.exists()
        || paths.backup_build_intent.exists()
        || paths.backup_stage_owner.exists()
        || paths.manifest_stage.exists()
    {
        return invalid("partial backup publication coexists with schema v5");
    }
    if paths.backup_final.exists() != paths.manifest_final.exists() {
        return invalid("retained backup pair is incomplete");
    }
    if paths.backup_final.exists() {
        verify_retained_pair(paths)?;
    }
    Ok(())
}

fn invalid<T>(message: &str) -> AppResult<T> {
    Err(AppError::InvalidRequest(message.into()))
}

use rusqlite::OptionalExtension;

#[cfg(test)]
pub(crate) mod tests;
