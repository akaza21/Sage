use std::{iter, ptr, thread, time::Duration};

use windows::Win32::{
    Foundation::{GlobalFree, HANDLE},
    System::{
        DataExchange::{
            CloseClipboard, EmptyClipboard, GetClipboardSequenceNumber, OpenClipboard,
            RegisterClipboardFormatW, SetClipboardData,
        },
        Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock},
        Ole::CF_UNICODETEXT,
    },
};
use windows_core::PCWSTR;
use zeroize::Zeroizing;

use crate::browser::passwords::{VaultError, VaultResult, secret::Secret};

/// Another app may hold the clipboard open for a moment.
const OPEN_ATTEMPTS: u32 = 10;
const OPEN_RETRY: Duration = Duration::from_millis(20);
/// Formats whose presence asks Windows to keep the copy out of clipboard
/// history, cloud sync and monitoring apps.
const EXCLUSIONS: [&str; 3] = [
    "ExcludeClipboardContentFromMonitorProcessing",
    "CanIncludeInClipboardHistory",
    "CanUploadToCloudClipboard",
];

/// The clipboard, opened without an owner window as `arboard` and
/// `clipboard-win` do, so any thread may use it.
struct OpenedClipboard;

impl OpenedClipboard {
    fn open() -> VaultResult<Self> {
        let mut attempt = 1;
        loop {
            // SAFETY: a missing owner is allowed.
            match unsafe { OpenClipboard(None) } {
                Ok(()) => return Ok(Self),
                Err(_) if attempt < OPEN_ATTEMPTS => {
                    attempt += 1;
                    thread::sleep(OPEN_RETRY);
                }
                Err(error) => return Err(VaultError::Io(error.into())),
            }
        }
    }
}

impl Drop for OpenedClipboard {
    fn drop(&mut self) {
        // SAFETY: this guard opened the clipboard on this thread.
        let _ = unsafe { CloseClipboard() };
    }
}

/// Hands `bytes` to the open clipboard as `format`; Windows owns the copy
/// once `SetClipboardData` succeeds.
fn set_data(format: u32, bytes: &[u8]) -> VaultResult<()> {
    let to_vault = |error: windows_core::Error| VaultError::Io(error.into());
    // SAFETY: the allocation is written only within its length, and freed
    // here unless the clipboard took ownership of it.
    unsafe {
        let memory = GlobalAlloc(GMEM_MOVEABLE, bytes.len()).map_err(to_vault)?;
        let target = GlobalLock(memory).cast::<u8>();
        if target.is_null() {
            let _ = GlobalFree(Some(memory));
            return Err(VaultError::Io(std::io::Error::last_os_error()));
        }
        ptr::copy_nonoverlapping(bytes.as_ptr(), target, bytes.len());
        // Unlocking the last lock reports an "error" with NO_ERROR.
        let _ = GlobalUnlock(memory);
        if let Err(error) = SetClipboardData(format, Some(HANDLE(memory.0))) {
            let _ = GlobalFree(Some(memory));
            return Err(to_vault(error));
        }
    }
    Ok(())
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(iter::once(0)).collect()
}

/// Copies `password` and returns the clipboard's sequence number after the
/// write, which `clear_if_unchanged` compares against. The exclusion formats
/// go in first, so the password is never on the clipboard without them; any
/// failure empties the clipboard again before it closes.
pub(in crate::browser::passwords) fn copy_concealed(password: &Secret) -> VaultResult<u32> {
    let text = Zeroizing::new(wide(password.expose()));
    let bytes: Zeroizing<Vec<u8>> =
        Zeroizing::new(text.iter().flat_map(|unit| unit.to_ne_bytes()).collect());
    {
        let _clipboard = OpenedClipboard::open()?;
        // SAFETY: the clipboard is open on this thread.
        unsafe { EmptyClipboard() }.map_err(|error| VaultError::Io(error.into()))?;
        let written = write_concealed(&bytes);
        if written.is_err() {
            // SAFETY: still open; leaves nothing half-marked behind.
            let _ = unsafe { EmptyClipboard() };
        }
        written?;
    }
    // Read after closing, so the number is the one later copies change.
    // SAFETY: no preconditions.
    Ok(unsafe { GetClipboardSequenceNumber() })
}

fn write_concealed(text: &[u8]) -> VaultResult<()> {
    for name in EXCLUSIONS {
        let name = wide(name);
        // SAFETY: `wide` returns a NUL-terminated UTF-16 string.
        let format = unsafe { RegisterClipboardFormatW(PCWSTR(name.as_ptr())) };
        if format == 0 {
            return Err(VaultError::Io(std::io::Error::last_os_error()));
        }
        set_data(format, &0u32.to_ne_bytes())?;
    }
    set_data(u32::from(CF_UNICODETEXT.0), text)
}

/// Empties the clipboard if nothing was copied since `sequence`.
pub(in crate::browser::passwords) fn clear_if_unchanged(sequence: u32) -> VaultResult<()> {
    // SAFETY: no preconditions.
    if unsafe { GetClipboardSequenceNumber() } != sequence {
        return Ok(());
    }
    let _clipboard = OpenedClipboard::open()?;
    // SAFETY: the clipboard is open; the sequence is checked again now that
    // no other app can change it.
    unsafe {
        if GetClipboardSequenceNumber() == sequence {
            EmptyClipboard().map_err(|error| VaultError::Io(error.into()))?;
        }
    }
    Ok(())
}
