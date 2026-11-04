//! Copy writes the password to the clipboard from Rust, marked so clipboard
//! history, cloud clipboard and clipboard monitors leave it out, and
//! attempts to clear it after `CLEAR_AFTER` if it still holds that same text.
//! Windows only for now, like the vault.

use std::{
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

use sha2::{Digest, Sha256};

use super::{VaultError, VaultResult};

pub(super) const CLEAR_AFTER: Duration = Duration::from_secs(30);
pub(super) const CLEAR_FAILED: &str = "Sage could not clear the copied password from the clipboard. Copy something else now to replace it.";
const SALT_LEN: usize = 16;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(super) use windows::{clear_if_unchanged, copy_concealed};

/// A salted fingerprint of the copied text and its length, so the later
/// clear empties the clipboard only if it still holds exactly that text,
/// reading no more of the clipboard than the copy wrote. Nothing that can
/// recover the password is kept.
#[derive(Clone)]
pub(super) struct CopiedText {
    salt: [u8; SALT_LEN],
    digest: [u8; 32],
    len: usize,
}

/// The copy whose clear is still due. The app owns it, so a password copied
/// shortly before Sage quits is cleared on the way out rather than left
/// behind with the timer that would have cleared it.
#[derive(Default)]
pub(super) struct PendingClear(Mutex<Option<Arc<CopiedText>>>);

impl PendingClear {
    /// Records `copied` as the copy to clear; it replaces any earlier one,
    /// whose text the new copy has already overwritten.
    fn track(&self, copied: CopiedText) -> Arc<CopiedText> {
        let copied = Arc::new(copied);
        *self.pending() = Some(Arc::clone(&copied));
        copied
    }

    /// Forgets `copied` once its own clear has run, unless a later copy has
    /// taken its place.
    pub(super) fn settle(&self, copied: &Arc<CopiedText>) {
        let mut pending = self.pending();
        if pending
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, copied))
        {
            *pending = None;
        }
    }

    /// Clears the copy whose clear is still due, if the clipboard still
    /// holds it. Quitting Sage calls this.
    pub(super) fn clear_now(
        &self,
        clipboard: impl FnMut(&CopiedText) -> VaultResult<()>,
    ) -> VaultResult<()> {
        let Some(copied) = self.pending().take() else {
            return Ok(());
        };
        clear_with_retries(&copied, clipboard)
    }

    fn pending(&self) -> std::sync::MutexGuard<'_, Option<Arc<CopiedText>>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The port must open, compare and conditionally clear atomically on every
/// attempt. A retry must never clear a newer copy using an earlier comparison.
fn clear_with_retries(
    copied: &CopiedText,
    mut clipboard: impl FnMut(&CopiedText) -> VaultResult<()>,
) -> VaultResult<()> {
    let mut result = clipboard(copied);
    // Four attempts with 700 ms of total backoff, off the UI thread.
    for backoff_ms in [100, 200, 400] {
        if result.is_ok() {
            break;
        }
        std::thread::sleep(Duration::from_millis(backoff_ms));
        result = clipboard(copied);
    }
    result
}

/// The warning port is native, so it still reaches the user after Settings
/// closes. Never log clipboard text or a fingerprint.
pub(super) fn expire(
    copied: &CopiedText,
    clipboard: impl FnMut(&CopiedText) -> VaultResult<()>,
    warn: impl FnOnce(&str),
) {
    if let Err(error) = clear_with_retries(copied, clipboard) {
        report_failure(&error.to_string(), warn);
    }
}

pub(super) fn report_failure(cause: &str, warn: impl FnOnce(&str)) {
    tracing::error!(cause, "Password clipboard expiry failed");
    warn(CLEAR_FAILED);
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

/// Writes `text` and registers its pending clear in the same step, on the
/// thread that owns the clipboard. App shutdown runs on that thread too, so
/// it can never find the password written without its clear registered.
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn write_and_track(
    pending: &PendingClear,
    text: &[u8],
    draw_salt: impl FnOnce(&mut [u8; SALT_LEN]) -> VaultResult<()>,
    write: impl FnOnce(&[u8]) -> VaultResult<()>,
) -> VaultResult<Arc<CopiedText>> {
    fingerprint_then_write(text, draw_salt, write).map(|copied| pending.track(copied))
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
    _pending: Arc<PendingClear>,
) -> VaultResult<Arc<CopiedText>> {
    Err(VaultError::Unsupported)
}

#[cfg(not(windows))]
pub(super) fn clear_if_unchanged(_copied: &CopiedText) -> VaultResult<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        cell::{Cell, RefCell},
        io::Write,
        sync::{Arc, Mutex},
    };

    use super::{
        CopiedText, PendingClear, VaultError, clear_with_retries, expire, fingerprint_then_write,
        random_salt, write_and_track,
    };

    struct Clipboard {
        text: Vec<u8>,
        busy: usize,
        replacement: Option<Vec<u8>>,
    }

    #[derive(Clone)]
    struct LogOutput(Arc<Mutex<Vec<u8>>>);

    impl Write for LogOutput {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Clipboard {
        fn clear_if_unchanged(&mut self, copied: &CopiedText) -> super::VaultResult<()> {
            if self.busy > 0 {
                self.busy -= 1;
                if let Some(text) = self.replacement.take() {
                    self.text = text;
                }
                return Err(VaultError::Invalid("The clipboard is busy."));
            }
            if copied.matches(&self.text) {
                self.text.clear();
            }
            Ok(())
        }
    }

    #[test]
    fn expiry_clears_a_password_when_a_busy_clipboard_becomes_free() {
        let mut clipboard = Clipboard {
            text: b"secret".to_vec(),
            busy: 1,
            replacement: None,
        };
        clear_with_retries(&copied(b"secret"), |copy| {
            clipboard.clear_if_unchanged(copy)
        })
        .unwrap();
        assert!(clipboard.text.is_empty());
    }

    #[test]
    fn expiry_reports_failure_when_the_clipboard_stays_busy() {
        let mut clipboard = Clipboard {
            text: b"secret".to_vec(),
            busy: usize::MAX,
            replacement: None,
        };
        let mut warning = String::new();
        let output = LogOutput(Arc::new(Mutex::new(Vec::new())));
        let writer = output.clone();
        let subscriber = tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .finish();
        tracing::subscriber::with_default(subscriber, || {
            expire(
                &copied(b"secret"),
                |copy| clipboard.clear_if_unchanged(copy),
                |message| warning = message.to_owned(),
            );
        });
        assert_eq!(
            warning,
            "Sage could not clear the copied password from the clipboard. Copy something else now to replace it."
        );
        assert_eq!(clipboard.text, b"secret");
        let bytes = output.0.lock().unwrap();
        let log = String::from_utf8_lossy(&bytes);
        assert!(log.contains("Password clipboard expiry failed"));
        assert!(log.contains("The clipboard is busy."));
        assert!(!log.contains("secret"));
    }

    #[test]
    fn expiry_leaves_a_newer_copy_untouched_after_a_busy_attempt() {
        let mut clipboard = Clipboard {
            text: b"secret".to_vec(),
            busy: 1,
            replacement: Some(b"new copy".to_vec()),
        };
        clear_with_retries(&copied(b"secret"), |copy| {
            clipboard.clear_if_unchanged(copy)
        })
        .unwrap();
        assert_eq!(clipboard.text, b"new copy");
    }

    /// Shutdown runs on the clipboard's thread as soon as the copy's step
    /// ends, before the copy command hears back: the clear is already due.
    #[test]
    fn quitting_right_after_the_write_clears_the_copied_password() {
        let clipboard = RefCell::new(Clipboard {
            text: b"older".to_vec(),
            busy: 0,
            replacement: None,
        });
        let pending = PendingClear::default();
        write_and_track(&pending, b"secret", random_salt, |text| {
            clipboard.borrow_mut().text = text.to_vec();
            Ok(())
        })
        .unwrap();
        pending
            .clear_now(|copy| clipboard.borrow_mut().clear_if_unchanged(copy))
            .unwrap();
        assert!(clipboard.borrow().text.is_empty());
    }

    #[test]
    fn a_copy_whose_clear_is_still_due_is_cleared_when_sage_quits() {
        let mut clipboard = Clipboard {
            text: b"secret".to_vec(),
            busy: 0,
            replacement: None,
        };
        let pending = PendingClear::default();
        pending.track(copied(b"secret"));
        pending
            .clear_now(|copy| clipboard.clear_if_unchanged(copy))
            .unwrap();
        assert!(clipboard.text.is_empty());
    }

    #[test]
    fn an_earlier_copys_expiry_leaves_the_later_copy_due_for_quitting() {
        let mut clipboard = Clipboard {
            text: b"second".to_vec(),
            busy: 0,
            replacement: None,
        };
        let pending = PendingClear::default();
        let first = pending.track(copied(b"first"));
        pending.track(copied(b"second"));
        pending.settle(&first);
        pending
            .clear_now(|copy| clipboard.clear_if_unchanged(copy))
            .unwrap();
        assert!(clipboard.text.is_empty());
    }

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
