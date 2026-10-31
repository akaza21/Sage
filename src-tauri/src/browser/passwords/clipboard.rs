//! Copy writes the password to the clipboard from Rust, marked so clipboard
//! history, cloud clipboard and clipboard monitors leave it out, and clears
//! it after `CLEAR_AFTER` if the clipboard still holds that same text.
//! Windows only for now, like the vault.

use std::time::Duration;

use sha2::{Digest, Sha256};

use super::{VaultError, VaultResult};

pub(super) const CLEAR_AFTER: Duration = Duration::from_secs(30);
const SALT_LEN: usize = 16;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(super) use windows::{clear_if_unchanged, copy_concealed};

/// A salted fingerprint of the copied text and its length, so the later
/// clear empties the clipboard only if it still holds exactly that text,
/// reading no more of the clipboard than the copy wrote. Nothing that can
/// recover the password is kept.
pub(super) struct CopiedText {
    salt: [u8; SALT_LEN],
    digest: [u8; 32],
    len: usize,
}

fn fingerprint(salt: &[u8; SALT_LEN], text: &[u8]) -> [u8; 32] {
    Sha256::new()
        .chain_update(salt)
        .chain_update(text)
        .finalize()
        .into()
}

/// Fingerprints `text` and only then hands it to `write`, so a failed salt
/// draw can never leave a password on the clipboard with no clear scheduled.
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn fingerprint_then_write(
    text: &[u8],
    draw_salt: impl FnOnce(&mut [u8; SALT_LEN]) -> VaultResult<()>,
    write: impl FnOnce(&[u8]) -> VaultResult<()>,
) -> VaultResult<CopiedText> {
    let mut salt = [0; SALT_LEN];
    draw_salt(&mut salt)?;
    let copied = CopiedText {
        digest: fingerprint(&salt, text),
        salt,
        len: text.len(),
    };
    write(text)?;
    Ok(copied)
}

#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn random_salt(salt: &mut [u8; SALT_LEN]) -> VaultResult<()> {
    getrandom::fill(salt).map_err(|_| VaultError::Random)
}

impl CopiedText {
    /// How many bytes of the clipboard the comparison needs.
    #[cfg_attr(not(any(windows, test)), allow(dead_code))]
    fn len(&self) -> usize {
        self.len
    }

    /// Whether `held`, at most `len()` bytes from the start of the
    /// clipboard's text, is the copied text. The copy ends in a NUL unit, so
    /// a longer text sharing its prefix cannot match.
    #[cfg_attr(not(any(windows, test)), allow(dead_code))]
    fn matches(&self, held: &[u8]) -> bool {
        held.len() == self.len && fingerprint(&self.salt, held) == self.digest
    }
}

#[cfg(not(windows))]
pub(super) fn copy_concealed(
    _owner: &tauri::Window,
    _password: super::secret::Secret,
) -> VaultResult<CopiedText> {
    Err(VaultError::Unsupported)
}

#[cfg(not(windows))]
pub(super) fn clear_if_unchanged(_owner: &tauri::Window, _copied: CopiedText) -> VaultResult<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::{CopiedText, VaultError, fingerprint_then_write, random_salt};

    fn copied(text: &[u8]) -> CopiedText {
        fingerprint_then_write(text, random_salt, |_| Ok(())).unwrap()
    }

    #[test]
    fn a_failed_salt_draw_writes_nothing() {
        let wrote = Cell::new(false);
        let result = fingerprint_then_write(
            b"p\0w\0\0\0",
            |_| Err(VaultError::Random),
            |_| {
                wrote.set(true);
                Ok(())
            },
        );
        assert!(matches!(result, Err(VaultError::Random)));
        assert!(!wrote.get());
    }

    #[test]
    fn only_the_exact_copied_text_matches() {
        let copied = copied(b"p\0w\0\0\0");
        assert_eq!(copied.len(), 6);
        assert!(copied.matches(b"p\0w\0\0\0"));
        assert!(!copied.matches(b"p\0x\0\0\0"));
        // A shorter clipboard is never read past its end, and the clear
        // reads only `len()` bytes of a longer one, which then ends early.
        assert!(!copied.matches(b"p\0w\0"));
        assert!(!copied.matches(b"p\0w\0x\0"));
        assert!(!copied.matches(b"p\0w\0\0\0\0\0"));
    }
}
