use std::{
    fs,
    io::{self, Read, Write},
    path::PathBuf,
    sync::Arc,
};

use zeroize::Zeroizing;

use super::{VaultError, VaultResult, cipher::VaultKey, file_identity::FileIdentity};

#[cfg(windows)]
mod dpapi;

/// A wrapped key file larger than this is not one Sage wrote.
const MAX_WRAPPED_KEY_BYTES: u64 = 4 * 1024;

/// Wraps the vault key with an OS secret that belongs to the signed-in user.
pub(super) trait KeyProtector: Send + Sync {
    fn protect(&self, key: &[u8]) -> io::Result<Vec<u8>>;
    /// `KeyLost` when the blob is damaged or belongs to another account;
    /// any other failure is `Io`, so a passing OS error never reads as a lost
    /// key the user should delete.
    fn unprotect(&self, wrapped: &[u8]) -> VaultResult<Zeroizing<Vec<u8>>>;
}

/// The platform's key protector, or `None` where Sage has not proven one yet.
pub(super) fn platform_protector() -> Option<Arc<dyn KeyProtector>> {
    #[cfg(windows)]
    {
        Some(Arc::new(dpapi::Dpapi))
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// The unwrapped key and the version of the key file it was read from.
pub(super) struct LoadedKey {
    key: VaultKey,
    file: FileIdentity,
}

impl LoadedKey {
    pub(super) fn key(&self) -> &VaultKey {
        &self.key
    }
}

/// The vault key's file in Sage's data folder. It only ever holds the key as
/// the protector wrapped it.
pub(super) struct KeyFile {
    path: PathBuf,
    protector: Arc<dyn KeyProtector>,
}

impl KeyFile {
    pub(super) fn new(path: PathBuf, protector: Arc<dyn KeyProtector>) -> Self {
        Self { path, protector }
    }

    /// The stored key, `None` when there is no key file yet. A file the
    /// protector cannot unwrap (another account, or damage) is `KeyLost`.
    pub(super) fn load(&self) -> VaultResult<Option<LoadedKey>> {
        let Some(file) = self.open()? else {
            return Ok(None);
        };
        let identity = FileIdentity::of(&file)?;
        let mut wrapped = Vec::new();
        file.take(MAX_WRAPPED_KEY_BYTES + 1)
            .read_to_end(&mut wrapped)?;
        if wrapped.len() as u64 > MAX_WRAPPED_KEY_BYTES {
            return Err(VaultError::KeyLost);
        }
        let key = self.protector.unprotect(&wrapped)?;
        let key = VaultKey::from_bytes(&key).ok_or(VaultError::KeyLost)?;
        Ok(Some(LoadedKey {
            key,
            file: identity,
        }))
    }

    /// Whether the key file is still the one `loaded` was read from.
    pub(super) fn still_holds(&self, loaded: &LoadedKey) -> VaultResult<bool> {
        Ok(match self.open()? {
            Some(file) => FileIdentity::of(&file)? == loaded.file,
            None => false,
        })
    }

    fn open(&self) -> VaultResult<Option<fs::File>> {
        match fs::File::open(&self.path) {
            Ok(file) => Ok(Some(file)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    /// Generates a key and writes it wrapped, through a temporary file so a
    /// crash never leaves a half-written key behind.
    pub(super) fn create(&self) -> VaultResult<LoadedKey> {
        let key = VaultKey::generate()?;
        let wrapped = self.protector.protect(key.as_bytes())?;
        let staging = self.path.with_extension("key-new");
        let mut file = fs::File::create(&staging)?;
        file.write_all(&wrapped)?;
        file.sync_all()?;
        // Renaming keeps the file's id, length and last write time.
        let identity = FileIdentity::of(&file)?;
        drop(file);
        fs::rename(&staging, &self.path)?;
        Ok(LoadedKey {
            key,
            file: identity,
        })
    }

    pub(super) fn remove(&self) -> VaultResult<()> {
        match fs::remove_file(&self.path) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error.into()),
            _ => Ok(()),
        }
    }
}
