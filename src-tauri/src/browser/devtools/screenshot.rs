use crate::acp::browser::{BrowserFailure, BrowserResult, validate_result};

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
