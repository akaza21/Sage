use std::{iter, ptr, slice, sync::mpsc};

use windows::Win32::{
    Foundation::{GlobalFree, HANDLE, HGLOBAL},
    System::{
        DataExchange::{
            CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable,
            OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
        },
        Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock},
        Ole::CF_UNICODETEXT,
    },
};
use windows_core::PCWSTR;
use zeroize::Zeroizing;

use super::{CopiedText, fingerprint_then_write, random_salt};
use crate::browser::passwords::{VaultError, VaultResult, secret::Secret};

/// Formats whose presence asks Windows to keep the copy out of clipboard
/// history, cloud sync and monitoring apps.
const EXCLUSIONS: [&str; 3] = [
    "ExcludeClipboardContentFromMonitorProcessing",
    "CanIncludeInClipboardHistory",
    "CanUploadToCloudClipboard",
];
fn io_error(error: windows_core::Error) -> VaultError {
    VaultError::Io(error.into())
}

/// Runs `task` on the UI thread that owns Sage's window, with the clipboard
/// opened for that window: Win32 refuses `SetClipboardData` after
/// `EmptyClipboard` on a clipboard opened without an owner.
fn with_clipboard<T: Send + 'static>(
    owner: &tauri::Window,
    task: impl FnOnce() -> VaultResult<T> + Send + 'static,
) -> VaultResult<T> {
    let (sender, receiver) = mpsc::sync_channel(1);
    let window = owner.clone();
    owner
        .run_on_main_thread(move || {
            let result = window
                .hwnd()
                .map_err(|_| VaultError::Invalid("Sage's window is not available."))
                .and_then(|hwnd| {
                    // SAFETY: `hwnd` is Sage's live main window, on its thread.
                    unsafe { OpenClipboard(Some(hwnd)) }.map_err(io_error)?;
                    let result = task();
                    // SAFETY: opened above on this thread.
                    let _ = unsafe { CloseClipboard() };
                    result
                });
            let _ = sender.send(result);
        })
        .map_err(|_| VaultError::Invalid("Sage's window is not available."))?;
    receiver
        .recv()
        .map_err(|_| VaultError::Invalid("Sage's window is not available."))?
}

/// Hands `bytes` to the open clipboard as `format`; Windows owns the copy
/// once `SetClipboardData` succeeds.
fn set_data(format: u32, bytes: &[u8]) -> VaultResult<()> {
    // SAFETY: the allocation is written only within its length, and freed
    // here unless the clipboard took ownership of it.
    unsafe {
        let memory = GlobalAlloc(GMEM_MOVEABLE, bytes.len()).map_err(io_error)?;
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
            return Err(io_error(error));
        }
    }
    Ok(())
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(iter::once(0)).collect()
}

/// The exclusion formats go in before the text, so the password is never on
/// the clipboard without them.
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

pub(in crate::browser::passwords) fn copy_concealed(
    owner: &tauri::Window,
    password: Secret,
) -> VaultResult<CopiedText> {
    with_clipboard(owner, move || {
        let text = Zeroizing::new(wide(password.expose()));
        let bytes: Zeroizing<Vec<u8>> =
            Zeroizing::new(text.iter().flat_map(|unit| unit.to_ne_bytes()).collect());
        fingerprint_then_write(&bytes, random_salt, |bytes| {
            // SAFETY: the clipboard is open on this thread.
            unsafe { EmptyClipboard() }.map_err(io_error)?;
            write_concealed(bytes).inspect_err(|_| {
                // SAFETY: still open; leaves nothing half-marked behind.
                let _ = unsafe { EmptyClipboard() };
            })
        })
    })
}

/// Whether the clipboard's Unicode text is still `copied`. Reads at most
/// the copy's own length, and nothing when the clipboard holds less.
///
/// # Safety
/// The clipboard must be open on this thread.
unsafe fn still_holds(copied: &CopiedText) -> VaultResult<bool> {
    // SAFETY: the caller holds the clipboard open; the handle stays valid
    // until it closes, and is read only within `GlobalSize`.
    unsafe {
        let format = u32::from(CF_UNICODETEXT.0);
        if IsClipboardFormatAvailable(format).is_err() {
            return Ok(false);
        }
        let handle = GetClipboardData(format).map_err(io_error)?;
        let memory = HGLOBAL(handle.0);
        if GlobalSize(memory) < copied.len() {
            return Ok(false);
        }
        let source = GlobalLock(memory).cast::<u8>();
        if source.is_null() {
            return Err(VaultError::Io(std::io::Error::last_os_error()));
        }
        let held = copied.matches(slice::from_raw_parts(source, copied.len()));
        let _ = GlobalUnlock(memory);
        Ok(held)
    }
}

/// Empties the clipboard if it still holds the text `copied` describes.
/// Emptying needs no owner window, so this runs on the calling thread and
/// still works while Sage quits, after its window is gone.
pub(in crate::browser::passwords) fn clear_if_unchanged(copied: &CopiedText) -> VaultResult<()> {
    // SAFETY: opened with no owner on this thread; emptying it then leaves
    // the clipboard with no owner, which is all a clear needs.
    unsafe { OpenClipboard(None) }.map_err(io_error)?;
    // SAFETY: the clipboard is open on this thread.
    let result = unsafe { still_holds(copied) }.and_then(|held| {
        if held {
            // SAFETY: as above.
            unsafe { EmptyClipboard() }.map_err(io_error)?;
        }
        Ok(())
    });
    // SAFETY: opened above on this thread.
    let _ = unsafe { CloseClipboard() };
    result
}
