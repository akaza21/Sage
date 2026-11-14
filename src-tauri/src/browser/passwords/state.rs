use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, MutexGuard, PoisonError, TryLockError, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use serde::Serialize;
use tauri::async_runtime::JoinHandle;

use super::{
    Cancellation, MAX_PASSWORD_BYTES, MAX_USERNAME_BYTES, VaultError, VaultResult,
    cipher::VaultKey,
    clipboard::{self, PendingClear},
    csv_import,
    imported_file::ImportedFile,
    key_store::{KeyFile, KeyProtector, LoadedKey, platform_protector},
    secret::Secret,
    store::{CredentialPage, MergeCounts, Source, Store},
    vault_lock::VaultLock,
};

const DATABASE_FILE: &str = "passwords.db";
const KEY_FILE: &str = "passwords.key";
const LOCK_FILE: &str = "passwords.lock";
/// A search longer than this cannot match a stored site or username.
const MAX_QUERY_BYTES: usize = 256;
/// How long the vault stays open after its last use. Closing it drops the
/// database connection and wipes the unwrapped key; the next use reopens it.
pub(super) const IDLE_AFTER: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "status")]
pub(crate) enum PasswordsOverview {
    Ready(CredentialPage),
    Unsupported,
}

/// Opened on first use so startup never touches the vault or the OS key
/// store.
#[derive(Clone)]
pub(crate) struct PasswordsState(Arc<Shared>);

struct Shared {
    directory: PathBuf,
    protector: Option<Arc<dyn KeyProtector>>,
    vault: Mutex<Option<OpenVault>>,
    /// Changed only while `vault` is locked.
    idle_watch: AtomicBool,
    clipboard: Arc<PendingClear>,
    copy_expiry: Mutex<Option<(u64, JoinHandle<()>)>>,
    imported_file: Mutex<Option<ImportedFile>>,
    /// Only one import runs at a time, so Cancel can never reach a later import.
    running_import: Mutex<Option<Arc<ImportStatus>>>,
}

struct ImportStatus {
    cancelled: AtomicBool,
    committed: Mutex<bool>,
}

/// Reserved before its file is chosen so a Cancel sent while the file dialog
/// is open still reaches it.
pub(super) struct ImportRun {
    state: PasswordsState,
    status: Arc<ImportStatus>,
}

impl ImportRun {
    pub(super) fn cancelled(&self) -> bool {
        self.status.cancelled.load(Ordering::Relaxed)
    }

    pub(super) fn import(&self, file: PathBuf) -> VaultResult<(MergeCounts, u32)> {
        let (opened, imported) = ImportedFile::open(file)?;
        let parsed = csv_import::read_file(opened, self)?;
        let counts = self.state.with_vault(|vault| {
            let Vault {
                store,
                key_file,
                key,
                ..
            } = vault;
            let key = unlocked(key, key_file, store, true)?;
            store.merge(key, Source::Csv, &parsed.credentials, self)
        })?;
        *lock(&self.state.0.imported_file) = Some(imported);
        Ok((counts, parsed.skipped))
    }
}

impl Cancellation for ImportRun {
    fn cancelled(&self) -> bool {
        self.cancelled()
    }

    fn commit(&self, write: impl FnOnce() -> VaultResult<()>) -> VaultResult<()> {
        let mut committed = lock(&self.status.committed);
        if self.cancelled() {
            return Err(VaultError::Cancelled);
        }
        write()?;
        *committed = true;
        Ok(())
    }
}

impl Drop for ImportRun {
    fn drop(&mut self) {
        lock(&self.state.0.running_import).take();
    }
}

struct OpenVault {
    lock: VaultLock,
    vault: Vault,
}

struct Vault {
    store: Store,
    key_file: KeyFile,
    key: Option<LoadedKey>,
    last_used: Instant,
}

impl Vault {
    fn drop_stale_key(&mut self) -> VaultResult<()> {
        if let Some(loaded) = &self.key
            && !self.key_file.still_holds(loaded)?
        {
            self.key = None;
        }
        Ok(())
    }
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
            idle_watch: AtomicBool::new(false),
            clipboard: Arc::default(),
            copy_expiry: Mutex::new(None),
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

    /// The action holds the cross-process vault lock, and a cached key another
    /// Sage process has since replaced or removed is dropped first, so the action
    /// never seals or opens a password with a key the vault no longer uses.
    fn with_vault<T>(&self, action: impl FnOnce(&mut Vault) -> VaultResult<T>) -> VaultResult<T> {
        let mut guard = lock(&self.0.vault);
        let OpenVault {
            lock: vault_lock,
            vault,
        } = match guard.as_mut() {
            Some(open) => open,
            None => {
                let protector = self.0.protector.clone().ok_or(VaultError::Unsupported)?;
                std::fs::create_dir_all(&self.0.directory)?;
                guard.insert(OpenVault {
                    lock: VaultLock::open(&self.0.directory.join(LOCK_FILE))?,
                    vault: Vault {
                        store: Store::open(&self.0.directory.join(DATABASE_FILE))?,
                        key_file: KeyFile::new(self.0.directory.join(KEY_FILE), protector),
                        key: None,
                        last_used: Instant::now(),
                    },
                })
            }
        };
        let held = vault_lock.acquire()?;
        let result = vault.drop_stale_key().and_then(|()| action(vault));
        drop(held);
        vault.last_used = Instant::now();
        if !self.0.idle_watch.swap(true, Ordering::Relaxed) {
            self.watch_idle();
        }
        result
    }

    fn watch_idle(&self) {
        let shared = Arc::downgrade(&self.0);
        let spawned = std::thread::Builder::new()
            .name("sage-passwords-idle".into())
            .spawn(move || {
                while let Some(wait) = Weak::upgrade(&shared)
                    .and_then(|shared| PasswordsState(shared).close_if_idle(Instant::now()))
                {
                    std::thread::sleep(wait);
                }
            });
        if let Err(error) = spawned {
            self.0.idle_watch.store(false, Ordering::Relaxed);
            tracing::warn!(%error, "Saved passwords idle watch could not start");
        }
    }

    fn close_if_idle(&self, now: Instant) -> Option<Duration> {
        let mut vault = lock(&self.0.vault);
        let idle_for = vault
            .as_ref()
            .map(|open| now.saturating_duration_since(open.vault.last_used));
        match idle_for {
            Some(idle_for) if idle_for < IDLE_AFTER => Some(IDLE_AFTER - idle_for),
            _ => {
                *vault = None;
                self.0.idle_watch.store(false, Ordering::Relaxed);
                None
            }
        }
    }

    #[cfg(test)]
    pub(super) fn close_if_idle_after(&self, elapsed: Duration) -> bool {
        self.close_if_idle(Instant::now() + elapsed).is_none()
    }

    #[cfg(test)]
    pub(super) fn is_open(&self) -> bool {
        lock(&self.0.vault).is_some()
    }

    pub(super) fn pending_clear(&self) -> Arc<PendingClear> {
        Arc::clone(&self.0.clipboard)
    }

    /// Copies can finish out of order, so a timer for a copy older than the
    /// current one is stopped instead.
    pub(super) fn replace_copy_expiry(&self, generation: u64, task: JoinHandle<()>) {
        let mut expiry = lock(&self.0.copy_expiry);
        match expiry.as_ref() {
            Some((current, _)) if *current > generation => task.abort(),
            _ => {
                if let Some((_, earlier)) = expiry.replace((generation, task)) {
                    earlier.abort();
                }
            }
        }
    }

    /// Called as Sage quits; failures are logged because no window is left to
    /// warn in.
    pub(crate) fn clear_clipboard_on_exit(&self) {
        if let Some((_, timer)) = lock(&self.0.copy_expiry).take() {
            timer.abort();
        }
        if let Err(error) =
            clipboard::clear_with_retries(|| clipboard::clear_due(&self.0.clipboard, None))
        {
            clipboard::report_failure(&error.to_string(), |_| {});
        }
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

    pub(super) fn begin_import(&self) -> VaultResult<ImportRun> {
        if !self.supported() {
            return Err(VaultError::Unsupported);
        }
        let mut running = lock(&self.0.running_import);
        if running.is_some() {
            return Err(VaultError::Invalid("An import is already running."));
        }
        let status = Arc::new(ImportStatus {
            cancelled: AtomicBool::new(false),
            committed: Mutex::new(false),
        });
        *running = Some(Arc::clone(&status));
        Ok(ImportRun {
            state: self.clone(),
            status,
        })
    }

    /// True acknowledges rollback; false means commit already won. A commit in
    /// progress is too late to cancel, so this request never waits for a durable
    /// write.
    pub(super) fn cancel_import_request(&self) -> impl FnOnce() -> bool + Send + 'static + use<> {
        let status = lock(&self.0.running_import).clone();
        move || {
            let Some(status) = status else {
                return false;
            };
            if status.cancelled.load(Ordering::Relaxed) {
                return true;
            }
            let committed = match status.committed.try_lock() {
                Ok(committed) => committed,
                Err(TryLockError::Poisoned(error)) => error.into_inner(),
                Err(TryLockError::WouldBlock) => return false,
            };
            if *committed {
                return false;
            }
            status.cancelled.store(true, Ordering::Relaxed);
            true
        }
    }

    #[cfg(test)]
    pub(super) fn cancel_import(&self) -> bool {
        self.cancel_import_request()()
    }

    pub(crate) fn delete_imported_file(&self) -> VaultResult<()> {
        let mut imported = lock(&self.0.imported_file);
        imported
            .as_ref()
            .ok_or(VaultError::Invalid("There is no imported file to delete."))?
            .delete()?;
        *imported = None;
        Ok(())
    }

    /// Checked first so Reveal and Copy do not ask the user to confirm for a
    /// password that cannot be read anyway.
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

    pub(crate) fn delete_all(&self) -> VaultResult<()> {
        self.with_vault(|vault| {
            vault.store.delete_all()?;
            vault.key = None;
            vault.key_file.remove()
        })
    }
}

/// A missing key is created only when `create` is set and no sealed row could
/// depend on an older one.
fn unlocked<'a>(
    key: &'a mut Option<LoadedKey>,
    key_file: &KeyFile,
    store: &Store,
    create: bool,
) -> VaultResult<&'a VaultKey> {
    match key {
        Some(loaded) => Ok(loaded.key()),
        None => {
            let loaded = match key_file.load()? {
                Some(loaded) => loaded,
                None if !store.is_empty()? => return Err(VaultError::KeyLost),
                None if create => key_file.create()?,
                None => return Err(VaultError::NotFound),
            };
            Ok(key.insert(loaded).key())
        }
    }
}
