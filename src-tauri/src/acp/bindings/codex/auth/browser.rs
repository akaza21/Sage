use super::NativeAuthUrl;
use crate::acp::bindings::codex::{AuthBrowser, CodexDiscoveryRunError};

pub(crate) struct NativeBrowser;

impl AuthBrowser for NativeBrowser {
    fn open(&self, url: &NativeAuthUrl) -> Result<(), CodexDiscoveryRunError> {
        open(url.as_str())
    }
}

#[cfg(windows)]
fn open(url: &str) -> Result<(), CodexDiscoveryRunError> {
    use windows::{Win32::UI::Shell::ShellExecuteW, core::PCWSTR};

    let operation = "open\0".encode_utf16().collect::<Vec<_>>();
    let target = format!("{url}\0").encode_utf16().collect::<Vec<_>>();
    // SAFETY: both pointers reference NUL-terminated UTF-16 buffers for the
    // duration of the synchronous ShellExecuteW call. No arguments or shell
    // command text are supplied.
    let result = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(operation.as_ptr()),
            PCWSTR(target.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
        )
    };
    if result.0 as isize <= 32 {
        Err(CodexDiscoveryRunError::Transport)
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn open(_: &str) -> Result<(), CodexDiscoveryRunError> {
    Err(CodexDiscoveryRunError::Unavailable)
}
