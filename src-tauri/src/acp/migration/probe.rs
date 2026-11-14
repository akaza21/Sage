use std::fs;

use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};

use sage_core::error::{AppError, AppResult};

use super::{
    backup::verify_sqlite,
    fingerprint::{
        DatabaseFingerprint, FileFingerprint, SchemaClass, database_fingerprint, file_fingerprint,
        open_read_only,
    },
    limits::{LIVE_SQLITE_BYTES, SIDE_FILES_BYTES, WAL_HARD_BYTES},
    manifest::{read, write_stage},
    paths::MigrationPaths,
    platform::{
        copy_bounded_create_new, file_is_prefix_of, reject_path_alias, remove_synced,
        require_free_bytes,
    },
};

mod snapshot;
pub(super) use snapshot::LiveSnapshot;

const PROBE_APP_IDENTITY: &str = "sage-ade-0006a-migration-probe-v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProbeIdentity {
    schema_version: u32,
    app_identity: String,
    source_main: FileFingerprint,
    source_wal: FileFingerprint,
    source_shm: FileFingerprint,
    probe_main_name: String,
    probe_wal_name: String,
    probe_shm_name: String,
}

pub(super) fn classify_live(paths: &MigrationPaths) -> AppResult<SchemaClass> {
    Ok(match inspect_live(paths)? {
        Some(database) => database.schema,
        None => SchemaClass::Fresh,
    })
}

pub(super) fn inspect_live(paths: &MigrationPaths) -> AppResult<Option<DatabaseFingerprint>> {
    with_live_snapshot(paths, |connection| {
        let database = database_fingerprint(connection)?;
        verify_sqlite(connection)?;
        Ok(database)
    })
}

/// Inspects committed WAL frames on a bounded copy, without opening the live
/// WAL or creating/updating its SHM. Recovery here requires prior version admission.
fn with_live_snapshot<T>(
    paths: &MigrationPaths,
    inspect: impl FnOnce(&Connection) -> AppResult<T>,
) -> AppResult<Option<T>> {
    paths.validate_containment()?;
    recover_probe(paths)?;
    let main = file_fingerprint(&paths.live)?;
    if !main.present || main.bytes == 0 {
        return Ok(None);
    }
    let wal = file_fingerprint(&paths.live_wal)?;
    if !wal.present || wal.bytes == 0 {
        let connection = open_read_only(&paths.live)?;
        return inspect(&connection).map(Some);
    }
    let main_before = main;
    let shm_before = file_fingerprint(&paths.live_shm)?;
    let required = main_before
        .bytes
        .saturating_add(wal.bytes)
        .saturating_add(SIDE_FILES_BYTES);
    require_free_bytes(paths.parent(), required)?;
    write_identity(paths, &main_before, &wal, &shm_before)?;

    let result = (|| {
        copy_bounded_create_new(&paths.live, &paths.probe_main, LIVE_SQLITE_BYTES)?;
        copy_bounded_create_new(&paths.live_wal, &paths.probe_wal, WAL_HARD_BYTES)?;
        verify_snapshot(paths, &main_before, &wal, &shm_before)?;
        inspect_snapshot(&paths.probe_main, inspect)
    })();
    let cleanup = cleanup_probe(paths);
    match (result, cleanup) {
        (Ok(database), Ok(())) => Ok(Some(database)),
        (Err(error), Ok(())) => Err(error),
        (_, Err(error)) => Err(error),
    }
}

fn inspect_snapshot<T>(
    path: &std::path::Path,
    inspect: impl FnOnce(&Connection) -> AppResult<T>,
) -> AppResult<T> {
    let anchor = super::platform::anchor_existing_sqlite(path, false)?;
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.pragma_update(None, "query_only", true)?;
    let result = inspect(&connection)?;
    connection.close().map_err(|(_, error)| error)?;
    anchor.verify_path(path)?;
    Ok(result)
}

pub(super) fn verify_snapshot(
    paths: &MigrationPaths,
    main_before: &FileFingerprint,
    wal_before: &FileFingerprint,
    shm_before: &FileFingerprint,
) -> AppResult<()> {
    verify_source(paths, main_before, wal_before, shm_before)?;
    verify_copy(&paths.probe_main, &paths.probe_wal, main_before, wal_before)
}

fn verify_copy(
    main: &std::path::Path,
    wal: &std::path::Path,
    main_before: &FileFingerprint,
    wal_before: &FileFingerprint,
) -> AppResult<()> {
    if file_fingerprint(main)? != *main_before || file_fingerprint(wal)? != *wal_before {
        return invalid("probe snapshot bytes do not match the live source");
    }
    Ok(())
}

fn verify_source(
    paths: &MigrationPaths,
    main_before: &FileFingerprint,
    wal_before: &FileFingerprint,
    shm_before: &FileFingerprint,
) -> AppResult<()> {
    let main_after = file_fingerprint(&paths.live)?;
    let wal_after = file_fingerprint(&paths.live_wal)?;
    let shm_after = file_fingerprint(&paths.live_shm)?;
    if *main_before != main_after || *wal_before != wal_after || *shm_before != shm_after {
        return invalid("live SQLite source changed during bounded probe snapshot");
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn write_current_identity(paths: &MigrationPaths) -> AppResult<()> {
    let main = file_fingerprint(&paths.live)?;
    let wal = file_fingerprint(&paths.live_wal)?;
    let shm = file_fingerprint(&paths.live_shm)?;
    write_identity(paths, &main, &wal, &shm)
}

fn write_identity(
    paths: &MigrationPaths,
    main: &FileFingerprint,
    wal: &FileFingerprint,
    shm: &FileFingerprint,
) -> AppResult<()> {
    if !main.present || !wal.present || wal.bytes == 0 {
        return invalid("probe identity requires a live main file and nonempty WAL");
    }
    let identity = ProbeIdentity {
        schema_version: 1,
        app_identity: PROBE_APP_IDENTITY.into(),
        source_main: main.clone(),
        source_wal: wal.clone(),
        source_shm: shm.clone(),
        probe_main_name: file_name(&paths.probe_main)?.into(),
        probe_wal_name: file_name(&paths.probe_wal)?.into(),
        probe_shm_name: file_name(&paths.probe_shm)?.into(),
    };
    write_stage(&paths.probe_manifest, &identity)
}

pub(super) fn recover_probe(paths: &MigrationPaths) -> AppResult<()> {
    if !paths.probe_manifest.exists() {
        return reject_unmanifested_probe(paths);
    }
    let identity: ProbeIdentity = read(&paths.probe_manifest)?;
    validate_identity(paths, &identity)?;
    if file_fingerprint(&paths.live)? != identity.source_main
        || file_fingerprint(&paths.live_wal)? != identity.source_wal
        || file_fingerprint(&paths.live_shm)? != identity.source_shm
    {
        return invalid("orphan probe identity no longer matches the live source");
    }
    validate_probe_prefix(paths, &identity)?;
    cleanup_probe(paths)
}

fn validate_identity(paths: &MigrationPaths, identity: &ProbeIdentity) -> AppResult<()> {
    if identity.schema_version != 1
        || identity.app_identity != PROBE_APP_IDENTITY
        || identity.probe_main_name != file_name(&paths.probe_main)?
        || identity.probe_wal_name != file_name(&paths.probe_wal)?
        || identity.probe_shm_name != file_name(&paths.probe_shm)?
        || !identity.source_main.present
        || !identity.source_wal.present
        || identity.source_wal.bytes == 0
    {
        return invalid("probe identity manifest is unsupported or corrupt");
    }
    Ok(())
}

fn validate_probe_prefix(paths: &MigrationPaths, identity: &ProbeIdentity) -> AppResult<()> {
    let main_present = paths.probe_main.exists();
    let wal_present = paths.probe_wal.exists();
    let shm_present = paths.probe_shm.exists();
    if !main_present {
        return if wal_present || shm_present {
            invalid("probe WAL/SHM exists before its main prefix")
        } else {
            Ok(())
        };
    }
    if !file_is_prefix_of(&paths.live, &paths.probe_main, LIVE_SQLITE_BYTES)? {
        return invalid("partial probe main is not a source prefix");
    }
    let main_complete = file_fingerprint(&paths.probe_main)? == identity.source_main;
    if !main_complete {
        return if wal_present || shm_present {
            invalid("probe WAL/SHM exists after an incomplete main prefix")
        } else {
            Ok(())
        };
    }
    if !wal_present {
        return if shm_present {
            invalid("probe SHM exists before its WAL prefix")
        } else {
            Ok(())
        };
    }
    if !file_is_prefix_of(&paths.live_wal, &paths.probe_wal, WAL_HARD_BYTES)? {
        return invalid("partial probe WAL is not a source prefix");
    }
    let wal_complete = file_fingerprint(&paths.probe_wal)? == identity.source_wal;
    if !wal_complete && shm_present {
        return invalid("probe SHM exists after an incomplete WAL prefix");
    }
    if shm_present {
        let shm = file_fingerprint(&paths.probe_shm)?;
        if shm.bytes > SIDE_FILES_BYTES {
            return Err(AppError::ResourceLimit(
                "probe SHM exceeds its physical limit".into(),
            ));
        }
    }
    Ok(())
}

fn reject_unmanifested_probe(paths: &MigrationPaths) -> AppResult<()> {
    for path in [&paths.probe_main, &paths.probe_wal, &paths.probe_shm] {
        reject_path_alias(path)?;
        match fs::symlink_metadata(path) {
            Ok(_) => return invalid("unmanifested migration probe artifact blocks startup"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn cleanup_probe(paths: &MigrationPaths) -> AppResult<()> {
    remove_synced(&paths.probe_shm)?;
    remove_synced(&paths.probe_wal)?;
    remove_synced(&paths.probe_main)?;
    remove_synced(&paths.probe_manifest)
}

fn file_name(path: &std::path::Path) -> AppResult<&str> {
    path.file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| AppError::InvalidRequest("probe filename is not UTF-8".into()))
}

fn invalid<T>(message: &str) -> AppResult<T> {
    Err(AppError::InvalidRequest(message.into()))
}
