//! Sage keeps its own vault rather than the engine's password manager: each
//! password is sealed with AES-256-GCM under a random vault key, stored only
//! wrapped by the OS. No reply but Reveal carries a password, and Reveal and
//! Copy first ask the OS to confirm the user.

mod cipher;
mod clipboard;
pub(crate) mod commands;
mod confirm;
mod csv_import;
#[cfg(test)]
mod csv_tests;
#[cfg(windows)]
mod engine;
mod file_identity;
mod imported_file;
mod key_store;
mod origin;
mod secret;
mod state;
mod store;
#[cfg(test)]
mod tests;
mod vault_lock;

#[cfg(windows)]
pub(super) use engine::disable_engine_password_manager;
pub(crate) use state::PasswordsState;

const MAX_USERNAME_BYTES: usize = 4 * 1024;
const MAX_PASSWORD_BYTES: usize = 1024;

#[derive(Debug, thiserror::Error)]
pub(crate) enum VaultError {
    #[error("Saved passwords are available only on Windows for now.")]
    Unsupported,
    #[error(
        "Saved passwords can't be read because the key that protects them is missing or belongs to another Windows account. Delete all saved passwords to start again."
    )]
    KeyLost,
    #[error("That saved password no longer exists.")]
    NotFound,
    #[error("A saved password is damaged and can't be read.")]
    Corrupt,
    #[error("Another saved password already uses that username for this site.")]
    Duplicate,
    #[error("{0}")]
    Invalid(&'static str),
    #[error("{0}")]
    Import(&'static str),
    #[error(
        "The imported file has been changed or replaced since the import, so Sage left it in place."
    )]
    ImportedFileChanged,
    #[error("The import was cancelled; nothing was saved.")]
    Cancelled,
    #[error("Set up Windows Hello to show or copy saved passwords.")]
    ConfirmationUnavailable,
    #[error("Windows didn't confirm it's you, so the password stays hidden.")]
    NotConfirmed,
    #[error("Sage could not generate a random key.")]
    Random,
    #[error("Saved password storage failed: {0}")]
    Storage(#[from] rusqlite::Error),
    #[error("Saved password storage failed: {0}")]
    Io(#[from] std::io::Error),
}

type VaultResult<T> = Result<T, VaultError>;

trait Cancellation {
    fn cancelled(&self) -> bool;

    /// Admission to the final write. ImportRun serializes this with Cancel;
    /// simple signals still get a final cancellation check before committing.
    fn commit(&self, write: impl FnOnce() -> VaultResult<()>) -> VaultResult<()>
    where
        Self: Sized,
    {
        if self.cancelled() {
            return Err(VaultError::Cancelled);
        }
        write()
    }
}

impl Cancellation for std::sync::atomic::AtomicBool {
    fn cancelled(&self) -> bool {
        self.load(std::sync::atomic::Ordering::Relaxed)
    }
}
