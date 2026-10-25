use serde_json::{Value, json};
use std::time::{Duration, Instant};

use crate::acp::{
    browser::{BrowserAction, BrowserFailure, BrowserResult, ViewportRegion, WaitCondition},
    limits::BROWSER_LIMITS,
};

use super::{
    capability::DevtoolsPort,
    screenshot::{admit_jpeg, admit_png, capture_png_with_fallback},
    snapshot::{DocumentStamp, NativeNode, parse_ax_tree},
    transport::CdpTransport,
};

mod act;
mod highlight;
pub(super) mod outcome;
pub(super) mod page_context;
mod world;
use outcome::{navigation_failure, navigation_matches};

/// The Agent browser command layer: CDP parameters, result parsing, caps and
/// document stamps, over any transport.
#[derive(Clone)]
pub(crate) struct CdpPort<T> {
    pub(super) transport: T,
    world: world::AgentWorld,
}

impl<T: CdpTransport> CdpPort<T> {
    pub(super) fn new(transport: T) -> Self {
        Self {
            transport,
            world: world::AgentWorld::default(),
        }
    }

    pub(super) fn call(
        &self,
        method: &str,
        params: &str,
        max_result_bytes: usize,
        timeout: Duration,
    ) -> Result<String, BrowserFailure> {
        self.transport
            .call(method, params, max_result_bytes, timeout)
    }

    pub(super) fn stamp(&self) -> Result<DocumentStamp, BrowserFailure> {
        self.transport.stamp()
    }

    /// Captures the user's own PNG of the page, only while the page still
    /// shows `expected_revision`.
    pub(super) fn capture_user_png(
        &mut self,
        expected_revision: &str,
    ) -> Result<(u32, u32, String), String> {
        let before = self.stamp().map_err(|_| "browser page unavailable")?;
        if before.generation.to_string() != expected_revision {
            return Err("browser page changed; retry screenshot".into());
        }
        let captured = capture_png_with_fallback(|scale| self.capture_image(None, "png", scale))
            .map_err(|error| match error {
                BrowserFailure::Capacity => "browser screenshot exceeds size limit",
                _ => "browser screenshot unavailable",
            })?;
        if self
            .stamp()
            .map_err(|_| "browser page changed; retry screenshot")?
            != before
        {
            return Err("browser page changed; retry screenshot".into());
        }
        Ok(captured)
    }

    fn capture_image(
        &mut self,
        region: Option<ViewportRegion>,
        format: &'static str,
        scale: f64,
    ) -> Result<(u32, u32, String), BrowserFailure> {
        let metrics = self.call(
            "Page.getLayoutMetrics",
            "{}",
            65_536,
            Duration::from_secs(5),
        )?;
        let metrics: Value =
            serde_json::from_str(&metrics).map_err(|_| BrowserFailure::Unavailable)?;
        let viewport = &metrics["cssVisualViewport"];
        let viewport_width = dimension(&viewport["clientWidth"])?;
        let viewport_height = dimension(&viewport["clientHeight"])?;
        let (x, y, width, height) = if let Some(region) = region {
            if region
                .x
                .checked_add(region.width)
                .is_none_or(|end| end > viewport_width)
                || region
                    .y
                    .checked_add(region.height)
                    .is_none_or(|end| end > viewport_height)
            {
                return Err(BrowserFailure::InvalidRequest);
            }
            (region.x, region.y, region.width, region.height)
        } else {
            (0, 0, viewport_width, viewport_height)
        };
        if width == 0
            || height == 0
            || width > BROWSER_LIMITS.screenshot_width
            || height > BROWSER_LIMITS.screenshot_height
            || u64::from(width) * u64::from(height) > BROWSER_LIMITS.screenshot_pixels
        {
            return Err(BrowserFailure::Capacity);
        }
        let params = json!({
            "format": format,
            "quality": 70,
            "fromSurface": true,
            "captureBeyondViewport": false,
            "clip": {"x": x, "y": y, "width": width, "height": height, "scale": scale},
        });
        let response = self.call(
            "Page.captureScreenshot",
            &params.to_string(),
            BROWSER_LIMITS.screenshot_bytes + 4096,
            Duration::from_secs(5),
        )?;
        let response: Value =
            serde_json::from_str(&response).map_err(|_| BrowserFailure::Unavailable)?;
        let data = response["data"]
            .as_str()
            .ok_or(BrowserFailure::Unavailable)?;
        if format == "png" {
            let (actual_width, actual_height) = admit_png(data)?;
            Ok((actual_width, actual_height, data.to_owned()))
        } else {
            admit_jpeg(width, height, data)?;
            Ok((width, height, data.to_owned()))
        }
    }
}

impl<T: CdpTransport> DevtoolsPort for CdpPort<T> {
    fn navigate(&mut self, url: &str) -> Result<(String, u64), BrowserFailure> {
        let before = self.stamp()?;
        // This CDP call goes through the dispatch permit and marks Agent
        // provenance before issue, just like the other Agent browser actions.
        let response = self
            .call(
                "Page.navigate",
                &json!({"url":url}).to_string(),
                4096,
                Duration::from_secs(5),
            )
            .map_err(|error| navigation_failure(self.transport.issued(), error))?;
        let response: Value =
            serde_json::from_str(&response).map_err(|_| BrowserFailure::OutcomeUnknown)?;
        if response.get("errorText").is_some() {
            return Err(BrowserFailure::OutcomeUnknown);
        }
        if response["frameId"].as_str().is_none() {
            return Err(BrowserFailure::OutcomeUnknown);
        }
        let issued_loader_id = response["loaderId"].as_str().map(str::to_owned);
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(settled) = self.transport.settled(before)? {
                let settled_loader_id = if issued_loader_id.is_some() {
                    let tree = self
                        .transport
                        .observe("Page.getFrameTree", "{}", 4096, Duration::from_secs(5))
                        .map_err(|_| BrowserFailure::OutcomeUnknown)?;
                    let tree: Value =
                        serde_json::from_str(&tree).map_err(|_| BrowserFailure::OutcomeUnknown)?;
                    tree["frameTree"]["frame"]["loaderId"]
                        .as_str()
                        .map(str::to_owned)
                } else {
                    None
                };
                if !navigation_matches(
                    url,
                    issued_loader_id.as_deref(),
                    &settled.url,
                    settled_loader_id.as_deref(),
                ) {
                    return Err(BrowserFailure::OutcomeUnknown);
                }
                return Ok((settled.url, settled.navigation_id));
            }
            if Instant::now() >= deadline {
                return Err(BrowserFailure::OutcomeUnknown);
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }
    fn snapshot(&mut self) -> Result<(DocumentStamp, Vec<NativeNode>), BrowserFailure> {
        let before = self.stamp()?;
        let payload = self.call(
            "Accessibility.getFullAXTree",
            r#"{"depth":16}"#,
            1_048_576,
            Duration::from_secs(5),
        )?;
        let nodes = parse_ax_tree(&payload)?;
        if self.stamp()? != before {
            return Err(BrowserFailure::StaleHandle);
        }
        Ok((before, nodes))
    }
    fn act(
        &mut self,
        stamp: DocumentStamp,
        backend_id: u64,
        action: &BrowserAction,
    ) -> Result<bool, BrowserFailure> {
        act::perform(self, stamp, backend_id, action)
    }
    fn capture(&mut self, region: Option<ViewportRegion>) -> Result<BrowserResult, BrowserFailure> {
        let (width, height, data) = self.capture_image(region, "jpeg", 1.0)?;
        Ok(BrowserResult::Screenshot {
            width,
            height,
            jpeg_base64: data,
        })
    }

    fn matches(
        &mut self,
        condition: &WaitCondition,
        remaining: Duration,
    ) -> Result<bool, BrowserFailure> {
        let WaitCondition::Text { text } = condition else {
            return Err(BrowserFailure::StaleHandle);
        };
        let needle = serde_json::to_string(text).map_err(|_| BrowserFailure::InvalidRequest)?;
        // A document still loading has no stamp yet; the next probe retries.
        let stamp = match self.stamp() {
            Err(BrowserFailure::Unavailable) => return Ok(false),
            stamp => stamp?,
        };
        let expression = format!("document.body?.innerText?.includes({needle}) === true");
        match self.evaluate_in_agent_world(stamp, &expression, 4096, remaining) {
            Ok(value) => Ok(value.as_bool().unwrap_or(false)),
            // The page navigated while the world was made; probe again.
            Err(BrowserFailure::StaleHandle) => Ok(false),
            Err(error) => Err(error),
        }
    }
}

fn dimension(value: &Value) -> Result<u32, BrowserFailure> {
    let value = value.as_f64().ok_or(BrowserFailure::Unavailable)?;
    if !value.is_finite() || value <= 0.0 || value > f64::from(u32::MAX) {
        return Err(BrowserFailure::Capacity);
    }
    Ok(value.ceil() as u32)
}
