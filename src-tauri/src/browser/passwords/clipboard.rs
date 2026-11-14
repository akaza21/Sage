//! Copy writes the password to the clipboard from Rust, marked so clipboard
//! history, cloud clipboard and clipboard monitors leave it out, and
//! attempts to clear it after `CLEAR_AFTER` if it still holds that same text.
//! Windows only for now, like the vault.

use std::{
    sync::{Mutex, MutexGuard, PoisonError},
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
pub(super) use windows::{clear_due, copy_concealed};

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

/// The latest password Copy wrote, while its clear is still due, and the
/// generation of that write. The app owns it, so a copy made shortly before
/// Sage quits is cleared on the way out, and only the latest write is ever
/// due: a timer for an earlier copy does nothing, even when both copies hold
/// the same password.
#[derive(Default)]
pub(super) struct PendingClear(Mutex<Slot>);

#[derive(Default)]
struct Slot {
    generation: u64,
    due: Option<CopiedText>,
}

impl PendingClear {
    /// Records `copied` as the latest write and returns its generation. The
    /// port calls this with the clipboard still open after the write, so the
    /// generation moves in step with Sage's clipboard writes.
    fn track(&self, copied: CopiedText) -> u64 {
        let mut slot = self.slot();
        slot.generation += 1;
        slot.due = Some(copied);
        slot.generation
    }

    /// Runs `clear` on the copy that is due, if it is the write `generation`
    /// names, or whichever is due when `generation` is `None` (quitting). The
    /// port calls this with the clipboard open, so no Copy can write between
    /// this check and the clear. A clear that succeeds settles the copy; one
    /// that fails leaves it due for a retry or for quitting.
    pub(super) fn clear_due(
        &self,
        generation: Option<u64>,
        clear: impl FnOnce(&CopiedText) -> VaultResult<()>,
    ) -> VaultResult<()> {
        let mut slot = self.slot();
        if generation.is_some_and(|generation| generation != slot.generation) {
            return Ok(());
        }
        let Some(copied) = &slot.due else {
            return Ok(());
        };
        clear(copied)?;
        slot.due = None;
        Ok(())
    }

    fn slot(&self) -> MutexGuard<'_, Slot> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Each attempt must open, check and conditionally clear the clipboard as
/// one step, so a retry never clears a newer copy using an earlier check.
pub(super) fn clear_with_retries(mut attempt: impl FnMut() -> VaultResult<()>) -> VaultResult<()> {
    let mut result = attempt();
    // Four attempts with 700 ms of total backoff.
    for backoff_ms in [100, 200, 400] {
        if result.is_ok() {
            break;
        }
        std::thread::sleep(Duration::from_millis(backoff_ms));
        result = attempt();
    }
    result
}

/// Clears copy `generation` if it is still the one due, retrying a busy
/// clipboard. The warning port is native, so it still reaches the user after
/// Settings closes. Never log clipboard text or a fingerprint.
pub(super) fn expire(attempt: impl FnMut() -> VaultResult<()>, warn: impl FnOnce(&str)) {
    if let Err(error) = clear_with_retries(attempt) {
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

/// Writes `text` and registers its pending clear in the same step, with the
/// clipboard open on the window's thread. App shutdown runs on that thread
/// too, so it can never find the password written without its clear due.
/// Returns the write's generation.
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn write_and_track(
    pending: &PendingClear,
    text: &[u8],
    draw_salt: impl FnOnce(&mut [u8; SALT_LEN]) -> VaultResult<()>,
    write: impl FnOnce(&[u8]) -> VaultResult<()>,
) -> VaultResult<u64> {
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
    _pending: std::sync::Arc<PendingClear>,
) -> VaultResult<u64> {
    Err(VaultError::Unsupported)
}

#[cfg(not(windows))]
pub(super) fn clear_due(_pending: &PendingClear, _generation: Option<u64>) -> VaultResult<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::{
        CopiedText, PendingClear, VaultError, VaultResult, clear_with_retries, expire,
        fingerprint_then_write, random_salt, write_and_track,
    };

    /// The OS clipboard as the port sees it: a busy clipboard refuses to
    /// open, and another program may write to it while Sage waits.
    struct Clipboard {
        text: Vec<u8>,
        busy: usize,
        replacement: Option<Vec<u8>>,
    }

    impl Clipboard {
        fn holding(text: &[u8]) -> Self {
            Self {
                text: text.to_vec(),
                busy: 0,
                replacement: None,
            }
        }

        /// What Copy does with the clipboard open.
        fn copy(&mut self, pending: &PendingClear, text: &[u8]) -> u64 {
            write_and_track(pending, text, random_salt, |text| {
                self.text = text.to_vec();
                Ok(())
            })
            .unwrap()
        }

        /// What the clear does with the clipboard open, as `clear_due` does.
        fn clear_due(
            &mut self,
            pending: &PendingClear,
            generation: Option<u64>,
        ) -> VaultResult<()> {
            if self.busy > 0 {
                self.busy -= 1;
                if let Some(text) = self.replacement.take() {
                    self.text = text;
                }
                return Err(VaultError::Invalid("The clipboard is busy."));
            }
            pending.clear_due(generation, |copied| {
                if copied.matches(&self.text) {
                    self.text.clear();
                }
                Ok(())
            })
        }
    }

    #[test]
    fn expiry_clears_a_password_when_a_busy_clipboard_becomes_free() {
        let pending = PendingClear::default();
        let mut clipboard = Clipboard::holding(b"");
        let generation = clipboard.copy(&pending, b"secret");
        clipboard.busy = 1;
        clear_with_retries(|| clipboard.clear_due(&pending, Some(generation))).unwrap();
        assert!(clipboard.text.is_empty());
    }

    #[test]
    fn expiry_reports_failure_when_the_clipboard_stays_busy() {
        let pending = PendingClear::default();
        let mut clipboard = Clipboard::holding(b"");
        let generation = clipboard.copy(&pending, b"secret");
        clipboard.busy = usize::MAX;
        let mut warning = String::new();
        let ((), log) = sage_test_support::test_diagnostics::capture_diagnostics(|| {
            expire(
                || clipboard.clear_due(&pending, Some(generation)),
                |message| warning = message.to_owned(),
            );
        });
        assert_eq!(
            warning,
            "Sage could not clear the copied password from the clipboard. Copy something else now to replace it."
        );
        assert_eq!(clipboard.text, b"secret");
        assert!(log.contains("Password clipboard expiry failed"));
        assert!(log.contains("The clipboard is busy."));
        assert!(!log.contains("secret"));
        // Still due, so quitting Sage tries again.
        clipboard.busy = 0;
        clipboard.clear_due(&pending, None).unwrap();
        assert!(clipboard.text.is_empty());
    }

    #[test]
    fn expiry_leaves_another_programs_newer_copy_untouched_after_a_busy_attempt() {
        let pending = PendingClear::default();
        let mut clipboard = Clipboard::holding(b"");
        let generation = clipboard.copy(&pending, b"secret");
        clipboard.busy = 1;
        clipboard.replacement = Some(b"new copy".to_vec());
        clear_with_retries(|| clipboard.clear_due(&pending, Some(generation))).unwrap();
        assert_eq!(clipboard.text, b"new copy");
    }

    /// Copy at 0 s and again at 20 s: the first copy's timer, firing at
    /// 30 s, must leave the second copy for its own timer at 50 s, even when
    /// both copies hold the same password.
    #[test]
    fn an_earlier_copys_timer_never_clears_a_later_copy() {
        let pending = PendingClear::default();
        let mut clipboard = Clipboard::holding(b"");
        let first = clipboard.copy(&pending, b"secret");
        let second = clipboard.copy(&pending, b"secret");
        clipboard.clear_due(&pending, Some(first)).unwrap();
        assert_eq!(clipboard.text, b"secret");
        clipboard.clear_due(&pending, Some(second)).unwrap();
        assert!(clipboard.text.is_empty());
    }

    /// Shutdown runs on the clipboard's thread as soon as the copy's step
    /// ends, before the copy command hears back: the clear is already due.
    #[test]
    fn quitting_right_after_the_write_clears_the_copied_password() {
        let pending = PendingClear::default();
        let mut clipboard = Clipboard::holding(b"older");
        clipboard.copy(&pending, b"secret");
        clipboard.clear_due(&pending, None).unwrap();
        assert!(clipboard.text.is_empty());
    }

    #[test]
    fn quitting_after_the_timer_cleared_leaves_a_later_copy_by_another_program() {
        let pending = PendingClear::default();
        let mut clipboard = Clipboard::holding(b"");
        let generation = clipboard.copy(&pending, b"secret");
        clipboard.clear_due(&pending, Some(generation)).unwrap();
        clipboard.text = b"secret".to_vec();
        clipboard.clear_due(&pending, None).unwrap();
        assert_eq!(clipboard.text, b"secret");
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
