use std::{
    io::Read,
    ops::Deref,
    path::{Path, PathBuf},
};

use rusqlite::{Connection, OpenFlags};
use sha2::{Digest, Sha256};

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct FileFingerprint {
    pub(super) present: bool,
    pub(super) bytes: u64,
    pub(super) sha256: Option<String>,
}

pub(super) struct AnchoredConnection {
    connection: Option<Connection>,
    _anchor: super::platform::SqlitePathAnchor,
    _path: PathBuf,
}

impl Deref for AnchoredConnection {
    type Target = Connection;

    fn deref(&self) -> &Self::Target {
        self.connection
            .as_ref()
            .expect("anchored connection is open")
    }
}

pub(super) fn file_fingerprint(path: &Path) -> AppResult<FileFingerprint> {
    file_fingerprint_with_cap(path, None)
}

#[cfg(test)]
pub(super) fn file_fingerprint_for_test(
    path: &Path,
    maximum_bytes: u64,
) -> AppResult<FileFingerprint> {
    file_fingerprint_with_cap(path, Some(maximum_bytes))
}

fn file_fingerprint_with_cap(
    path: &Path,
    maximum_bytes: Option<u64>,
) -> AppResult<FileFingerprint> {
    super::platform::reject_path_alias(path)?;
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(FileFingerprint {
                present: false,
                bytes: 0,
                sha256: None,
            });
        }
        Err(error) => return Err(error.into()),
    };
    let cap = match maximum_bytes {
        Some(value) => value,
        None => artifact_cap(path)?,
    };
    if metadata.len() > cap {
        return limit("migration artifact exceeds its physical byte limit");
    }
    let mut file = match super::platform::open_read_no_follow(path) {
        Ok(file) => file,
        Err(AppError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(FileFingerprint {
                present: false,
                bytes: 0,
                sha256: None,
            });
        }
        Err(error) => return Err(error),
    };
    let mut hash = Sha256::new();
    let mut bytes = 0_u64;
    let mut buffer = [0_u8; 1_048_576];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        bytes = bytes.saturating_add(read as u64);
        if bytes > cap {
            return limit("migration artifact grew beyond its physical byte limit");
        }
        hash.update(&buffer[..read]);
    }
    Ok(FileFingerprint {
        present: true,
        bytes,
        sha256: Some(format!("{:x}", hash.finalize())),
    })
}

pub(super) fn open_read_only(path: &Path) -> AppResult<AnchoredConnection> {
    super::platform::reject_path_alias(path)?;
    require_read_only_wal_state(path)?;
    let anchor = super::platform::anchor_existing_sqlite(path, false)?;
    let uri = read_only_uri(path)?;
    let connection = Connection::open_with_flags(
        uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_URI,
    )?;
    connection.pragma_update(None, "query_only", true)?;
    anchor.verify_path(path)?;
    Ok(AnchoredConnection {
        connection: Some(connection),
        _anchor: anchor,
        _path: path.to_path_buf(),
    })
}

fn read_only_uri(path: &Path) -> AppResult<String> {
    let text = path.to_str().ok_or_else(|| {
        AppError::InvalidRequest("SQLite migration path is not valid UTF-8".into())
    })?;
    let normalized = text.replace('\\', "/");
    let mut encoded = String::with_capacity(normalized.len() + 32);
    for byte in normalized.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b':' | b'-' | b'_' | b'.' | b'~') {
            encoded.push(char::from(byte));
        } else {
            use std::fmt::Write;
            write!(&mut encoded, "%{byte:02X}").expect("writing to String cannot fail");
        }
    }
    Ok(format!("file:{encoded}?mode=ro&immutable=1"))
}

fn require_read_only_wal_state(path: &Path) -> AppResult<()> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| AppError::InvalidRequest("SQLite database filename is not UTF-8".into()))?;
    let wal = path.with_file_name(format!("{name}-wal"));
    let shm = path.with_file_name(format!("{name}-shm"));
    super::platform::reject_path_alias(&wal)?;
    super::platform::reject_path_alias(&shm)?;
    let wal_bytes = std::fs::metadata(&wal)
        .map(|value| value.len())
        .unwrap_or(0);
    if wal_bytes > 0 {
        return invalid(
            "nonempty WAL requires an offline checkpoint before immutable schema migration",
        );
    }
    Ok(())
}

fn artifact_cap(path: &Path) -> AppResult<u64> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            AppError::InvalidRequest("migration artifact filename is not UTF-8".into())
        })?;
    if matches!(
        name,
        "sage.v5.db.stage-journal" | "sage.pre-v5.db.stage-journal"
    ) {
        Ok(super::limits::JOURNAL_BYTES)
    } else if name.ends_with("-wal") || name.contains("db-wal.retired") {
        Ok(super::limits::WAL_HARD_BYTES)
    } else if name.ends_with("-shm") || name.contains("db-shm.retired") {
        Ok(super::limits::SIDE_FILES_BYTES)
    } else if matches!(
        name,
        "sage.db"
            | "sage.pre-v5.db.stage"
            | "sage.pre-v5.db"
            | "sage.v5.db.stage"
            | "sage.v4.db.retired"
            | "sage.migration-probe.db"
    ) {
        Ok(super::limits::LIVE_SQLITE_BYTES)
    } else {
        invalid("unrecognized migration artifact filename")
    }
}

fn invalid<T>(message: &str) -> AppResult<T> {
    Err(AppError::InvalidRequest(message.into()))
}

fn limit<T>(message: &str) -> AppResult<T> {
    Err(AppError::ResourceLimit(message.into()))
}
