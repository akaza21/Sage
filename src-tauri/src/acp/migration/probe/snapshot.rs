use std::{fs, path::PathBuf};

use sha2::{Digest, Sha256};

use super::*;

/// A disposable WAL snapshot, isolated from durable recovery artifacts. Only
/// fixed, bounded files in one owned temporary directory per profile belong to
/// it, including across crashes. The profile's migration lock serializes reuse.
pub(in super::super) struct LiveSnapshot {
    directory: PathBuf,
    paths: MigrationPaths,
    source: [FileFingerprint; 3],
}

const OWNER_DIRECTORY: &str = "sage-migration-probe-v1.owner";

impl LiveSnapshot {
    pub(in super::super) fn capture(paths: &MigrationPaths) -> AppResult<Self> {
        paths.validate_containment()?;
        let main = file_fingerprint(&paths.live)?;
        let wal = file_fingerprint(&paths.live_wal)?;
        let shm = file_fingerprint(&paths.live_shm)?;
        let temporary = std::env::temp_dir();
        reject_path_alias(&temporary)?;
        let live = fs::canonicalize(&paths.live)?;
        let key = Sha256::digest(live.as_os_str().as_encoded_bytes());
        let directory = temporary.join(format!("sage-migration-probe-{key:x}"));
        claim_directory(&directory)?;
        let snapshot = Self {
            directory,
            paths: paths.clone(),
            source: [main, wal, shm],
        };
        // Reclaim the old owned slot before measuring space for its replacement.
        require_free_bytes(
            &temporary,
            snapshot.source[0]
                .bytes
                .saturating_add(snapshot.source[1].bytes)
                .saturating_add(SIDE_FILES_BYTES),
        )?;
        copy_bounded_create_new(&paths.live, &snapshot.main(), LIVE_SQLITE_BYTES)?;
        copy_bounded_create_new(&paths.live_wal, &snapshot.wal(), WAL_HARD_BYTES)?;
        snapshot.verify_source()?;
        verify_copy(
            &snapshot.main(),
            &snapshot.wal(),
            &snapshot.source[0],
            &snapshot.source[1],
        )?;
        Ok(snapshot)
    }

    pub(in super::super) fn inspect<T>(
        &self,
        inspect: impl FnOnce(&Connection) -> AppResult<T>,
    ) -> AppResult<T> {
        inspect_snapshot(&self.main(), inspect)
    }

    pub(in super::super) fn classify(&self) -> AppResult<SchemaClass> {
        self.inspect(|connection| {
            let database = database_fingerprint(connection)?;
            verify_sqlite(connection)?;
            Ok(database.schema)
        })
    }

    pub(in super::super) fn verify_source(&self) -> AppResult<()> {
        verify_source(
            &self.paths,
            &self.source[0],
            &self.source[1],
            &self.source[2],
        )
    }

    fn main(&self) -> PathBuf {
        self.directory.join("sage.migration-probe.db")
    }

    fn wal(&self) -> PathBuf {
        self.directory.join("sage.migration-probe.db-wal")
    }
}

impl Drop for LiveSnapshot {
    fn drop(&mut self) {
        let _ = cleanup_directory(&self.directory);
    }
}

fn claim_directory(directory: &std::path::Path) -> AppResult<()> {
    reject_path_alias(directory)?;
    let owner = directory.join(OWNER_DIRECTORY);
    match create_private_directory(directory) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            verify_private_directory(directory)?;
            reject_path_alias(&owner)?;
            match fs::metadata(&owner) {
                Ok(metadata) if metadata.is_dir() => {
                    // This disposable scratch is outside the profile: reclaiming
                    // it cannot change artifacts requiring version admission.
                    return cleanup_files(directory);
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    // Interrupted claim/cleanup may leave an empty directory.
                    // remove_dir refuses any unowned contents without a walk.
                    fs::remove_dir(directory)?;
                    create_private_directory(directory)?;
                }
                Ok(_) => return invalid("temporary probe owner is not a directory"),
                Err(error) => return Err(error.into()),
            }
        }
        Err(error) => return Err(error.into()),
    }
    // An atomic directory marker cannot be left as a partially written manifest.
    fs::create_dir(&owner)?;
    super::super::platform::sync_parent(&owner)?;
    Ok(())
}

fn create_private_directory(directory: &std::path::Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new().mode(0o700).create(directory)
    }
    #[cfg(not(unix))]
    fs::create_dir(directory)
}

fn verify_private_directory(directory: &std::path::Path) -> AppResult<()> {
    let metadata = fs::symlink_metadata(directory)?;
    if !metadata.is_dir() {
        return invalid("temporary probe path is not a directory");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        // SAFETY: geteuid has no arguments or memory-safety preconditions.
        let current_user = unsafe { libc::geteuid() };
        if metadata.uid() != current_user || metadata.mode() & 0o077 != 0 {
            return invalid("temporary probe directory is not private to this user");
        }
    }
    Ok(())
}

fn cleanup_files(directory: &std::path::Path) -> AppResult<()> {
    // Never enumerate or recursively delete a directory of unknown files.
    for name in [
        "sage.migration-probe.db-shm",
        "sage.migration-probe.db-wal",
        "sage.migration-probe.db",
    ] {
        remove_synced(&directory.join(name))?;
    }
    Ok(())
}

fn cleanup_directory(directory: &std::path::Path) -> AppResult<()> {
    cleanup_files(directory)?;
    fs::remove_dir(directory.join(OWNER_DIRECTORY))?;
    fs::remove_dir(directory)?;
    Ok(())
}

#[cfg(test)]
mod tests;
