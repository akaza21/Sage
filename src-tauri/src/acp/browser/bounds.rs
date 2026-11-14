use super::BrowserFailure;
use sage_core::limits::BROWSER_LIMITS;

pub(crate) fn admit_text(field: &str, value: &str) -> Result<(), BrowserFailure> {
    let cap = match field {
        "sessionId" => BROWSER_LIMITS.session_id_bytes,
        "url" => BROWSER_LIMITS.url_bytes,
        "handle" => BROWSER_LIMITS.handle_bytes,
        "key" => BROWSER_LIMITS.key_bytes,
        "fill" => BROWSER_LIMITS.fill_bytes,
        "waitText" => BROWSER_LIMITS.wait_text_bytes,
        "snapshotText" => BROWSER_LIMITS.snapshot_text_bytes,
        "nodeRole" => BROWSER_LIMITS.node_role_bytes,
        "nodeName" => BROWSER_LIMITS.node_name_bytes,
        "diagnosticText" => BROWSER_LIMITS.diagnostic_text_bytes,
        "diagnosticLevel" => BROWSER_LIMITS.diagnostic_level_bytes,
        _ => return Err(BrowserFailure::InvalidRequest),
    };
    if value.len() > cap {
        return Err(BrowserFailure::Capacity);
    }
    Ok(())
}

pub(crate) fn admit_count(count: usize, cap: usize) -> Result<(), BrowserFailure> {
    if count > cap {
        Err(BrowserFailure::Capacity)
    } else {
        Ok(())
    }
}

pub(crate) fn admit_screenshot(
    width: u32,
    height: u32,
    bytes: usize,
) -> Result<(), BrowserFailure> {
    let limits = BROWSER_LIMITS;
    if width == 0 || height == 0 {
        return Err(BrowserFailure::InvalidRequest);
    }
    if width > limits.screenshot_width
        || height > limits.screenshot_height
        || bytes > limits.screenshot_bytes
    {
        return Err(BrowserFailure::Capacity);
    }
    admit_capture_pixels(f64::from(width), f64::from(height), 1.0).map(|_| ())
}

/// Bounds the bitmap before native capture and after image decoding. Round
/// each side outward before multiplying, without integer casts or overflow.
pub(crate) fn admit_capture_pixels(
    width: f64,
    height: f64,
    scale: f64,
) -> Result<(f64, f64), BrowserFailure> {
    if [width, height, scale]
        .iter()
        .any(|value| !value.is_finite() || *value <= 0.0)
    {
        return Err(BrowserFailure::InvalidRequest);
    }
    let (width, height) = (
        (width * scale).ceil().max(1.0),
        (height * scale).ceil().max(1.0),
    );
    if width * height > BROWSER_LIMITS.screenshot_pixels as f64 {
        return Err(BrowserFailure::Capacity);
    }
    Ok((width, height))
}
