mod backup;
mod build_intent;
mod file_artifact;
mod fingerprint;
mod identity;
mod legacy;
mod legacy_events;
mod legacy_project;
mod limits;
mod manifest;
mod paths;
mod platform;
mod platform_anchor;
mod probe;
mod publish;
mod stage_owner;
#[cfg(test)]
mod test_failpoint;

#[allow(unused_imports)]
pub(crate) use identity::legacy_binding_digest;

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
};

#[cfg(test)]
use self::manifest::{ReplacementManifest, write_stage};

const INITIAL: &str = include_str!("../../../migrations/001_initial.sql");
const V2: &str = include_str!("../../../migrations/002_diff_tabs.sql");
const V3: &str = include_str!("../../../migrations/003_editor_buffers.sql");

pub(crate) struct OpenedDatabase {
    connection: Connection,
    live_path_anchor: LivePathAnchor,
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
        build_fresh_v5(&mut connection)?;
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
            build_fresh_v5(&mut connection)?;
            verify_current(&connection)?;
            anchor.verify_path(path)?;
            Ok(opened_database(connection, anchor))
        }
        SchemaClass::CurrentV5 => {
            reject_partial_backup_publication(&paths)?;
            let anchor = platform::anchor_existing_sqlite(path, true)?;
            if probe::classify_live(&paths)? != SchemaClass::CurrentV5 {
                return invalid("current SQLite path changed before its anchored open");
            }
            let connection = Connection::open(path)?;
            verify_current(&connection)?;
            anchor.verify_path(path)?;
            Ok(opened_database(connection, anchor))
        }
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
    apply_through_v5(&mut destination)?;
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

#[cfg(test)]
fn inject_replacement_build_failure_once() {
    test_failpoint::arm("replacement-build");
}

fn fail_replacement_build_if_injected() -> AppResult<()> {
    #[cfg(test)]
    test_failpoint::hit("replacement-build")?;
    Ok(())
}

fn apply_through_v5(connection: &mut Connection) -> AppResult<()> {
    let versions = connection
        .prepare("SELECT version FROM schema_migrations ORDER BY version LIMIT 7")?
        .query_map([], |row| row.get::<_, i64>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    if versions.is_empty()
        || versions.len() > 4
        || versions
            .iter()
            .enumerate()
            .any(|(index, version)| *version != index as i64 + 1)
    {
        return invalid("replacement source has unsupported migration history");
    }
    let last = versions.last().copied().unwrap_or(0);
    if last < 2 {
        connection.execute_batch(V2)?;
    }
    if last < 3 {
        connection.execute_batch(V3)?;
    }
    if last < 4 {
        project_migration::migrate(connection)?;
    }
    apply_v5(connection)
}

fn apply_v5(connection: &mut Connection) -> AppResult<()> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    legacy::convert(&transaction, now_millis())?;
    let foreign_key_error = transaction
        .query_row("PRAGMA foreign_key_check", [], |_| Ok(true))
        .optional()?
        .unwrap_or(false);
    if foreign_key_error {
        return invalid("ACP migration failed its foreign-key check");
    }
    transaction.commit()?;
    verify_current(connection)
}

fn build_fresh_v5(connection: &mut Connection) -> AppResult<()> {
    connection.execute_batch(INITIAL)?;
    connection.execute_batch(V2)?;
    connection.execute_batch(V3)?;
    project_migration::migrate(connection)?;
    apply_v5(connection)
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
    if fingerprint::classify_connection(connection)? != SchemaClass::CurrentV5 {
        return invalid("database did not reach exact schema v5");
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
