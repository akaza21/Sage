//! Copy writes the password to the clipboard from Rust, marked so clipboard
//! history, cloud clipboard and clipboard monitors leave it out, and clears
//! it after `CLEAR_AFTER` if the clipboard still holds that same text.
//! Windows only for now, like the vault.

use std::time::Duration;

pub(super) const CLEAR_AFTER: Duration = Duration::from_secs(30);

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(super) use windows::{clear_if_unchanged, copy_concealed};

#[cfg(not(windows))]
pub(super) struct CopiedText;

#[cfg(not(windows))]
pub(super) fn copy_concealed(
    _owner: &tauri::Window,
    _password: super::secret::Secret,
) -> super::VaultResult<CopiedText> {
    Err(super::VaultError::Unsupported)
}

#[cfg(not(windows))]
pub(super) fn clear_if_unchanged(
    _owner: &tauri::Window,
    _copied: CopiedText,
) -> super::VaultResult<()> {
    Ok(())
}
