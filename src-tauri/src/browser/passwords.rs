//! Saved browser passwords (BF-7b-1). Sage keeps its own vault rather than
//! the engine's password manager: each password is sealed with AES-256-GCM
//! under a random vault key, and that key is stored only wrapped by the OS
//! (DPAPI on Windows). macOS and Linux report the vault as unsupported until
//! their key stores are proven. No reply but Reveal carries a password, and
//! Reveal and Copy first ask the OS to confirm the user.

mod cipher;
mod clipboard;
pub(crate) mod commands;
mod confirm;
mod csv_import;
#[cfg(test)]
mod csv_tests;
#[cfg(windows)]
mod engine;
mod key_store;
mod origin;
mod secret;
mod state;
mod store;
#[cfg(test)]
mod tests;

#[cfg(windows)]
pub(super) use engine::disable_engine_password_manager;
pub(crate) use state::PasswordsState;

/// The longest username Sage stores, in UTF-8 bytes.
const MAX_USERNAME_BYTES: usize = 4 * 1024;
/// The longest password Sage stores, in UTF-8 bytes.
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

/// Whether the operation that owns this signal was cancelled. Long imports
/// check it between reads and between rows.
trait Cancellation {
    fn cancelled(&self) -> bool;
}

impl Cancellation for std::sync::atomic::AtomicBool {
    fn cancelled(&self) -> bool {
        self.load(std::sync::atomic::Ordering::Relaxed)
    }
}
