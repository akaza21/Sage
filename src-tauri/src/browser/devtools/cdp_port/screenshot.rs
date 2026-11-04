//! Resolve screenshot geometry before dispatch. Explicit clips pin the
//! admitted size even if page content grows after the metrics read.
use serde_json::{Map, Value, json};
use std::time::{Duration, Instant};

use super::{CdpPort, CdpTransport, world::left};
use crate::acp::browser::{BrowserFailure, developer::ScreenshotCapture};

impl<T: CdpTransport> CdpPort<T> {
    pub(super) fn bounded_screenshot(
        &self,
        params: &str,
        deadline: Instant,
    ) -> Result<String, BrowserFailure> {
        let mut params: Map<String, Value> =
            serde_json::from_str(params).map_err(|_| BrowserFailure::InvalidRequest)?;
        let capture = ScreenshotCapture::parse(&params)?;
        let before = self.stamp()?;
        let clip = match capture.clip {
            Some(clip) => clip,
            None => {
                let metrics = self.transport.observe(
                    "Page.getLayoutMetrics",
                    "{}",
                    65_536,
                    left(deadline)?.min(Duration::from_secs(5)),
                )?;
                let metrics: Value =
                    serde_json::from_str(&metrics).map_err(|_| BrowserFailure::Unavailable)?;
                let clip = if capture.beyond_viewport {
                    let content = &metrics["cssContentSize"];
                    json!({"x": content["x"], "y": content["y"],
                        "width": content["width"], "height": content["height"], "scale": 1})
                } else {
                    let viewport = &metrics["cssVisualViewport"];
                    json!({"x": viewport["pageX"], "y": viewport["pageY"],
                        "width": viewport["clientWidth"], "height": viewport["clientHeight"], "scale": 1})
                };
                serde_json::from_value(clip).map_err(|_| BrowserFailure::Unavailable)?
            }
        };
        clip.admit(1.0)?;
        // This trusted isolated-world read cannot be replaced by page script.
        // Device emulation and zoom can multiply the bitmap beyond CSS size.
        let ratio = self.evaluate_in_agent_world(
            Some(before.navigation_id),
            "window.devicePixelRatio",
            4096,
            deadline.min(Instant::now() + Duration::from_secs(5)),
        )?;
        let ratio = ratio.as_f64().ok_or(BrowserFailure::Unavailable)?;
        clip.admit(ratio)?;
        if self.stamp()? != before {
            return Err(BrowserFailure::StaleHandle);
        }
        params.insert(
            "clip".into(),
            serde_json::to_value(clip).map_err(|_| BrowserFailure::InvalidRequest)?,
        );
        serde_json::to_string(&params).map_err(|_| BrowserFailure::InvalidRequest)
    }
}

#[cfg(test)]
#[path = "screenshot_tests.rs"]
mod tests;
