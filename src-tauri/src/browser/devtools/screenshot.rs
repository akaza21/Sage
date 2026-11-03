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

/// A PNG's size from its header, read from the first 32 base64 characters.
pub(super) fn png_size(png_base64: &str) -> Option<(u32, u32)> {
    let header = STANDARD.decode(png_base64.get(..32)?).ok()?;
    if !header.starts_with(b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR") {
        return None;
    }
    let width = u32::from_be_bytes(header[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(header[20..24].try_into().ok()?);
    Some((width, height))
}

/// The factor that brings a picture over the size limits inside them, or
/// `None` when it already fits. Rounded down so rounding cannot overshoot.
pub(super) fn fit_factor(width: u32, height: u32) -> Option<f64> {
    let limits = crate::acp::limits::BROWSER_LIMITS;
    let (width, height) = (f64::from(width.max(1)), f64::from(height.max(1)));
    let fit = (f64::from(limits.screenshot_width) / width)
        .min(f64::from(limits.screenshot_height) / height)
        .min((limits.screenshot_pixels as f64 / (width * height)).sqrt());
    (fit < 1.0).then(|| (fit * 1000.0).floor() / 1000.0)
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

#[cfg(test)]
mod tests {
    use super::{STANDARD, fit_factor, png_size};
    use base64::Engine;

    #[test]
    fn png_size_reads_the_header_and_rejects_anything_else() {
        let mut header = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR".to_vec();
        header.extend(980u32.to_be_bytes());
        header.extend(1743u32.to_be_bytes());
        header.extend([8, 6, 0, 0, 0]);
        assert_eq!(png_size(&STANDARD.encode(&header)), Some((980, 1743)));
        assert_eq!(
            png_size(&STANDARD.encode(b"/9j/not a png at all, padded..")),
            None
        );
        assert_eq!(png_size("iVBOR"), None);
    }

    #[test]
    fn fit_factor_brings_an_oversized_picture_inside_every_limit() {
        assert_eq!(fit_factor(1920, 1080), None);
        for (width, height) in [(1960, 3486), (3840, 2160), (4000, 100), (100, 9000)] {
            let fit = fit_factor(width, height).expect("oversized");
            let (w, h) = (
                (f64::from(width) * fit).ceil(),
                (f64::from(height) * fit).ceil(),
            );
            assert!(
                w <= 1920.0 && h <= 1080.0 && w * h <= 2_073_600.0,
                "{w}x{h}"
            );
        }
    }
}
