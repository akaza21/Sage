//! Two Sage processes can open the same profile, so every vault operation
//! holds an exclusive lock on `passwords.lock` beside the vault. While one
//! process creates, replaces or removes the key, no other can check its
//! cached key against the key file or seal a password with it.

use std::{
    fs::{File, OpenOptions},
    io,
    path::Path,
};

/// The open lock file. Opening it takes no lock; `acquire` does.
pub(super) struct VaultLock(File);

/// Holds the lock until dropped.
pub(super) struct VaultLockGuard<'a>(&'a File);

impl VaultLock {
    pub(super) fn open(path: &Path) -> io::Result<Self> {
        OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map(Self)
    }

    /// Waits until no other process holds the lock, then takes it.
    pub(super) fn acquire(&self) -> io::Result<VaultLockGuard<'_>> {
        self.0.lock()?;
        Ok(VaultLockGuard(&self.0))
    }
}

impl Drop for VaultLockGuard<'_> {
    fn drop(&mut self) {
        // Closing the file at the latest releases the lock too.
        let _ = self.0.unlock();
    }
}
