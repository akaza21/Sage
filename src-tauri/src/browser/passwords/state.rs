use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
};

use serde::Serialize;

use super::{
    MAX_PASSWORD_BYTES, MAX_USERNAME_BYTES, VaultError, VaultResult,
    cipher::VaultKey,
    csv_import,
    key_store::{KeyFile, KeyProtector, platform_protector},
    secret::Secret,
    store::{CredentialPage, MergeCounts, Source, Store},
};

const DATABASE_FILE: &str = "passwords.db";
const KEY_FILE: &str = "passwords.key";
/// A search longer than this cannot match a stored site or username.
const MAX_QUERY_BYTES: usize = 256;

/// The Passwords page's view of the vault.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "status")]
pub(crate) enum PasswordsOverview {
    Ready(CredentialPage),
    Unsupported,
}

/// Saved passwords for the whole app, opened on first use so startup never
/// touches the vault or the OS key store.
#[derive(Clone)]
pub(crate) struct PasswordsState(Arc<Shared>);

struct Shared {
    directory: PathBuf,
    protector: Option<Arc<dyn KeyProtector>>,
    vault: Mutex<Option<Vault>>,
    /// The CSV the last import read, kept so the page can offer to delete it.
    imported_file: Mutex<Option<PathBuf>>,
    /// The running import's own cancel flag, if one is running. Only one
    /// import runs at a time, so Cancel can never reach a later import.
    running_import: Mutex<Option<Arc<AtomicBool>>>,
}

/// One admitted import, reserved before its file is chosen so a Cancel sent
/// while the file dialog is open still reaches it. Dropping it, however the
/// import ends, lets the next one in.
pub(super) struct ImportRun {
    state: PasswordsState,
    cancel: Arc<AtomicBool>,
}

impl ImportRun {
    pub(super) fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// Reads `file` and merges it into the vault in one transaction. Returns
    /// the merge counts and how many rows could not be used.
    pub(super) fn import(&self, file: PathBuf) -> VaultResult<(MergeCounts, u32)> {
        let cancel = self.cancel.as_ref();
        let parsed = csv_import::read_file(&file, cancel)?;
        let counts = self.state.with_vault(|vault| {
            let Vault {
                store,
                key_file,
                key,
            } = vault;
            let key = unlocked(key, key_file, store, true)?;
            store.merge(key, Source::Csv, &parsed.credentials, cancel)
        })?;
        *lock(&self.state.0.imported_file) = Some(file);
        Ok((counts, parsed.skipped))
    }
}

impl Drop for ImportRun {
    fn drop(&mut self) {
        lock(&self.state.0.running_import).take();
    }
}

struct Vault {
    store: Store,
    key_file: KeyFile,
    key: Option<VaultKey>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl PasswordsState {
    pub(crate) fn new(directory: &Path) -> Self {
        Self::with_protector(directory, platform_protector())
    }

    fn with_protector(directory: &Path, protector: Option<Arc<dyn KeyProtector>>) -> Self {
        Self(Arc::new(Shared {
            directory: directory.to_path_buf(),
            protector,
            vault: Mutex::new(None),
            imported_file: Mutex::new(None),
            running_import: Mutex::new(None),
        }))
    }

    #[cfg(test)]
    pub(super) fn for_test(directory: &Path, protector: Arc<dyn KeyProtector>) -> Self {
        Self::with_protector(directory, Some(protector))
    }

    pub(crate) fn supported(&self) -> bool {
        self.0.protector.is_some()
    }

    fn with_vault<T>(&self, action: impl FnOnce(&mut Vault) -> VaultResult<T>) -> VaultResult<T> {
        let mut vault = lock(&self.0.vault);
        if vault.is_none() {
            let protector = self.0.protector.clone().ok_or(VaultError::Unsupported)?;
            std::fs::create_dir_all(&self.0.directory)?;
            *vault = Some(Vault {
                store: Store::open(&self.0.directory.join(DATABASE_FILE))?,
                key_file: KeyFile::new(self.0.directory.join(KEY_FILE), protector),
                key: None,
            });
        }
        action(vault.as_mut().ok_or(VaultError::Unsupported)?)
    }

    pub(crate) fn overview(&self, query: &str) -> VaultResult<PasswordsOverview> {
        if !self.supported() {
            return Ok(PasswordsOverview::Unsupported);
        }
        if query.len() > MAX_QUERY_BYTES {
            return Err(VaultError::Invalid("That search is too long."));
        }
        self.with_vault(|vault| vault.store.page(query).map(PasswordsOverview::Ready))
    }

    /// Admits one import; refuses while another is running.
    pub(super) fn begin_import(&self) -> VaultResult<ImportRun> {
        if !self.supported() {
            return Err(VaultError::Unsupported);
        }
        let mut running = lock(&self.0.running_import);
        if running.is_some() {
            return Err(VaultError::Invalid("An import is already running."));
        }
        let cancel = Arc::new(AtomicBool::new(false));
        *running = Some(Arc::clone(&cancel));
        Ok(ImportRun {
            state: self.clone(),
            cancel,
        })
    }

    /// Cancels the running import, if any; a later import starts afresh.
    pub(crate) fn cancel_import(&self) {
        if let Some(cancel) = lock(&self.0.running_import).as_ref() {
            cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Deletes the CSV the last import read: it holds every password in
    /// plain text. Only that one file can be deleted this way.
    pub(crate) fn delete_imported_file(&self) -> VaultResult<()> {
        let file = lock(&self.0.imported_file)
            .take()
            .ok_or(VaultError::Invalid("There is no imported file to delete."))?;
        std::fs::remove_file(file)?;
        Ok(())
    }

    /// Checks that `id` exists and its key unwraps, so Reveal and Copy do not
    /// ask the user to confirm for a password that cannot be read anyway.
    pub(crate) fn check_readable(&self, id: &str) -> VaultResult<()> {
        self.with_vault(|vault| {
            if !vault.store.contains(id)? {
                return Err(VaultError::NotFound);
            }
            unlocked(&mut vault.key, &vault.key_file, &vault.store, false).map(drop)
        })
    }

    pub(crate) fn password(&self, id: &str) -> VaultResult<Secret> {
        self.with_vault(|vault| {
            let key = unlocked(&mut vault.key, &vault.key_file, &vault.store, false)?;
            vault.store.password(key, id)
        })
    }

    pub(crate) fn update(
        &self,
        id: &str,
        username: &str,
        password: Option<&Secret>,
    ) -> VaultResult<()> {
        if username.len() > MAX_USERNAME_BYTES {
            return Err(VaultError::Invalid("That username is too long."));
        }
        if let Some(password) = password {
            match password.expose().len() {
                0 => return Err(VaultError::Invalid("Enter a password.")),
                length if length > MAX_PASSWORD_BYTES => {
                    return Err(VaultError::Invalid("That password is too long."));
                }
                _ => {}
            }
        }
        self.with_vault(|vault| {
            let key = unlocked(&mut vault.key, &vault.key_file, &vault.store, false)?;
            vault.store.update(key, id, username, password)
        })
    }

    pub(crate) fn delete(&self, id: &str) -> VaultResult<()> {
        self.with_vault(|vault| vault.store.delete(id))
    }

    /// Removes every saved password and the key that sealed them, so a vault
    /// whose key was lost can start again.
    pub(crate) fn delete_all(&self) -> VaultResult<()> {
        self.with_vault(|vault| {
            vault.store.delete_all()?;
            vault.key = None;
            vault.key_file.remove()
        })
    }
}

/// The vault key, loaded once per run. A missing key is created only when
/// `create` is set and no sealed row could depend on an older one.
fn unlocked<'a>(
    key: &'a mut Option<VaultKey>,
    key_file: &KeyFile,
    store: &Store,
    create: bool,
) -> VaultResult<&'a VaultKey> {
    match key {
        Some(key) => Ok(key),
        None => {
            let loaded = match key_file.load()? {
                Some(loaded) => loaded,
                None if !store.is_empty()? => return Err(VaultError::KeyLost),
                None if create => key_file.create()?,
                None => return Err(VaultError::NotFound),
            };
            Ok(key.insert(loaded))
        }
    }
}
