//! Opens a web page in the user's own browser, outside Sage, for a
//! sign-in a harness asks the user to complete.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BrowserError {
    /// This system has no way to open the user's browser from Sage.
    Unavailable,
    /// The system refused to open the page.
    Failed,
}

/// Opens an https page. Anything else is refused, so a caller cannot
/// launch a program or a local file through this.
pub(crate) fn open_https(url: &str) -> Result<(), BrowserError> {
    let admitted = !url.chars().any(char::is_control)
        && url::Url::parse(url).is_ok_and(|parsed| {
            parsed.scheme() == "https" && parsed.host_str().is_some_and(|host| !host.is_empty())
        });
    if !admitted {
        return Err(BrowserError::Failed);
    }
    open(url)
}

#[cfg(windows)]
fn open(url: &str) -> Result<(), BrowserError> {
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
        Err(BrowserError::Failed)
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn open(_: &str) -> Result<(), BrowserError> {
    Err(BrowserError::Unavailable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_https_page_is_opened() {
        for refused in [
            "",
            "http://example.com/",
            "file:///c:/windows/system32/calc.exe",
            "ms-settings:privacy",
            "https://example.com/\r\nx",
            "notaurl",
        ] {
            assert_eq!(open_https(refused), Err(BrowserError::Failed), "{refused}");
        }
    }
}
