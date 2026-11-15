#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BrowserError {
    #[cfg_attr(
        windows,
        expect(dead_code, reason = "Windows always has a way to open the browser")
    )]
    Unavailable,
    Failed,
}

/// Opens an https page. Anything else is refused, so a caller cannot
/// launch a program or a local file through this.
pub(crate) fn open_https(url: &str) -> Result<(), BrowserError> {
    if url.chars().any(char::is_control) {
        return Err(BrowserError::Failed);
    }
    let parsed = url::Url::parse(url).map_err(|_| BrowserError::Failed)?;
    if parsed.scheme() != "https" || parsed.host_str().is_none_or(str::is_empty) {
        return Err(BrowserError::Failed);
    }
    // The system gets the URL as parsed, never the text it was parsed from.
    open(parsed.as_str())
}

pub(crate) const fn available() -> bool {
    cfg!(windows)
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
