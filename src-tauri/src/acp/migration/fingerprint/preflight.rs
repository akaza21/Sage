use std::{io::Read, path::Path};

use rusqlite::Connection;

use crate::error::{AppError, AppResult};

use super::super::{limits, paths::MigrationPaths, platform, probe};
use super::{
    open_read_only,
    version::{reject_newer_history, reject_version},
};

/// Refuses newer profiles before replacement recovery. Resource limits and
/// unreadable WAL snapshots fail closed; other main-file errors may need recovery.
pub(in super::super) fn reject_newer_live(
    paths: &MigrationPaths,
) -> AppResult<Option<probe::LiveSnapshot>> {
    match std::fs::metadata(&paths.live) {
        Ok(metadata) if metadata.len() == 0 => return Ok(None),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
        _ => {}
    }
    // Admit physical sizes before any SQLite query, even for malformed history.
    limits::verify_file_envelope(&paths.live)?;
    let has_wal = match std::fs::metadata(&paths.live_wal) {
        Ok(metadata) => metadata.len() != 0,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(error.into()),
    };
    let newer = (|| {
        if let Some(stored) = header_user_version(&paths.live)? {
            reject_version(i64::from(stored))?;
        }
        if has_wal {
            let snapshot = probe::LiveSnapshot::capture(paths)?;
            snapshot.inspect(inspect_version)?;
            // A refused profile must retain even a valid interrupted probe.
            probe::recover_probe(paths)?;
            Ok(Some(snapshot))
        } else {
            // The immutable open is cheap here and cannot create live sidecars.
            let connection = open_read_only(&paths.live)?;
            inspect_version(&connection)?;
            Ok(None)
        }
    })();
    match newer {
        // Recovery must not act on a live WAL whose version could not be read.
        Err(error) if has_wal => Err(error),
        Err(error @ (AppError::NewerProfile { .. } | AppError::ResourceLimit(_))) => Err(error),
        Ok(snapshot) => Ok(snapshot),
        Err(_) => Ok(None),
    }
}

fn inspect_version(connection: &Connection) -> AppResult<()> {
    let marker: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    reject_version(marker)?;
    let tracked: bool = connection.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM sqlite_schema WHERE type='table' AND name='schema_migrations'
         )",
        [],
        |row| row.get(0),
    )?;
    if tracked {
        reject_newer_history(connection)?;
    }
    Ok(())
}

/// Read a checkpointed marker without opening SQLite; WAL-only markers are
/// inspected through the probe instead.
fn header_user_version(path: &Path) -> AppResult<Option<i32>> {
    const MAGIC: &[u8; 16] = b"SQLite format 3\0";
    platform::reject_path_alias(path)?;
    let mut header = [0_u8; 64];
    match std::fs::File::open(path).and_then(|mut file| file.read_exact(&mut header)) {
        Ok(()) if header.starts_with(MAGIC) => Ok(Some(i32::from_be_bytes([
            header[60], header[61], header[62], header[63],
        ]))),
        Ok(()) => Ok(None),
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::UnexpectedEof
            ) =>
        {
            Ok(None)
        }
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests;
