//! Screenshot parameters that determine native allocation, before any
//! response cap can help. Dynamic page dimensions are resolved by CdpPort.
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::acp::browser::{BrowserFailure, admit_capture_pixels};

/// Avoid scales that collapse to zero in Chromium's native float geometry.
const MIN_CAPTURE_SCALE: f64 = 0.01;

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CaptureClip {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub scale: f64,
}

impl CaptureClip {
    pub(crate) fn admit(&self, pixel_ratio: f64) -> Result<(), BrowserFailure> {
        if !self.x.is_finite()
            || !self.y.is_finite()
            || !(self.x + self.width).is_finite()
            || !(self.y + self.height).is_finite()
            || !self.scale.is_finite()
            || self.scale <= 0.0
            || !pixel_ratio.is_finite()
            || pixel_ratio <= 0.0
        {
            return Err(BrowserFailure::InvalidRequest);
        }
        // Downscaling is allowed; magnification is never needed to capture
        // the page and can inflate Chromium's intermediate render surfaces.
        if !(MIN_CAPTURE_SCALE..=1.0).contains(&self.scale) {
            return Err(BrowserFailure::Capacity);
        }
        admit_capture_pixels(self.width, self.height, self.scale * pixel_ratio)?;
        Ok(())
    }
}

#[cfg_attr(not(any(windows, test)), allow(dead_code))]
pub(crate) struct ScreenshotCapture {
    pub clip: Option<CaptureClip>,
    pub beyond_viewport: bool,
}

impl ScreenshotCapture {
    pub(crate) fn parse(params: &Map<String, Value>) -> Result<Self, BrowserFailure> {
        let flag = |key: &str, default| match params.get(key) {
            None => Ok(default),
            Some(value) => value.as_bool().ok_or(BrowserFailure::InvalidRequest),
        };
        // Chromium ignores clip and device emulation for window captures.
        // Page metrics cannot bound that native window's bitmap; fail closed.
        if !flag("fromSurface", true)? {
            return Err(BrowserFailure::Denied);
        }
        let beyond_viewport = flag("captureBeyondViewport", false)?;
        let clip = params
            .get("clip")
            .map(|clip| serde_json::from_value::<CaptureClip>(clip.clone()))
            .transpose()
            .map_err(|_| BrowserFailure::InvalidRequest)?;
        if let Some(clip) = &clip {
            clip.admit(1.0)?;
        }
        Ok(Self {
            clip,
            beyond_viewport,
        })
    }
}
