use base64::{Engine, engine::general_purpose::STANDARD};

use crate::acp::browser::{BrowserFailure, BrowserResult, admit_screenshot, validate_result};

pub(super) fn capture_png_with_fallback<T>(
    mut attempt: impl FnMut(f64) -> Result<T, BrowserFailure>,
) -> Result<T, BrowserFailure> {
    for scale in [1.0, 0.75, 0.5] {
        match attempt(scale) {
            Err(BrowserFailure::Capacity) if scale != 0.5 => continue,
            result => return result,
        }
    }
    Err(BrowserFailure::Capacity)
}

pub(super) fn admit_png(png_base64: &str) -> Result<(u32, u32), BrowserFailure> {
    if png_base64.len() > crate::acp::limits::BROWSER_LIMITS.screenshot_bytes {
        return Err(BrowserFailure::Capacity);
    }
    let bytes = STANDARD
        .decode(png_base64)
        .map_err(|_| BrowserFailure::InvalidRequest)?;
    if STANDARD.encode(&bytes) != png_base64
        || !bytes.starts_with(b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR")
        || bytes.len() < 24
    {
        return Err(BrowserFailure::InvalidRequest);
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
    let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
    admit_screenshot(width, height, png_base64.len())?;
    Ok((width, height))
}

pub(super) fn admit_jpeg(width: u32, height: u32, jpeg_base64: &str) -> Result<(), BrowserFailure> {
    if !jpeg_base64.starts_with("/9j/")
        || !jpeg_base64.len().is_multiple_of(4)
        || !jpeg_base64
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'='))
    {
        return Err(BrowserFailure::InvalidRequest);
    }
    validate_result(&BrowserResult::Screenshot {
        width,
        height,
        jpeg_base64: jpeg_base64.to_owned(),
    })
}
