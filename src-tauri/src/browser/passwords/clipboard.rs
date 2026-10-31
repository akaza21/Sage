//! Copy writes the password to the clipboard from Rust, marked so clipboard
//! history, cloud clipboard and clipboard monitors leave it out, and clears
//! it after `CLEAR_AFTER` if it still holds the password. Windows only for
//! now, like the vault.

use std::time::Duration;

pub(super) const CLEAR_AFTER: Duration = Duration::from_secs(30);

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(super) use windows::{clear_if_unchanged, copy_concealed};

#[cfg(not(windows))]
pub(super) fn copy_concealed(_password: &super::secret::Secret) -> super::VaultResult<u32> {
    Err(super::VaultError::Unsupported)
}

#[cfg(not(windows))]
pub(super) fn clear_if_unchanged(_sequence: u32) -> super::VaultResult<()> {
    Ok(())
}
