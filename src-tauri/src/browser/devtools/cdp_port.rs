use serde_json::{Value, json};
use std::time::{Duration, Instant};

use crate::acp::browser::{
    BrowserAction, BrowserFailure, BrowserResult, ViewportRegion, WaitCondition,
};
use sage_core::limits::BROWSER_LIMITS;

use super::{
    capability::DevtoolsPort,
    screenshot::{admit_jpeg, admit_png, capture_png_with_fallback, fit_factor, png_size},
    snapshot::{DocumentStamp, NativeNode, parse_ax_tree},
    transport::CdpTransport,
};

mod act;
pub(super) mod evaluate;
pub(super) mod highlight;
pub(super) mod inspect;
pub(super) mod outcome;
pub(super) mod page_context;
mod screenshot;
pub(super) mod select;
mod world;
use outcome::{navigation_failure, navigation_matches};

/// The least time a wait probe gets, whatever is left of the wait.
const PROBE_FLOOR: Duration = Duration::from_secs(1);

/// The Agent browser command layer: CDP parameters, result parsing, caps and
/// document stamps, over any transport.
#[derive(Clone)]
pub(crate) struct CdpPort<T> {
    pub(super) transport: T,
    world: world::AgentWorld,
    pub(super) serializer: evaluate::Serializer,
}

impl<T: CdpTransport> CdpPort<T> {
    pub(super) fn new(transport: T) -> Self {
        Self {
            transport,
            world: world::AgentWorld::default(),
            serializer: evaluate::Serializer::default(),
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
        // No capture shows the Agent's act marker, even one taken while an
        // act is under way.
        highlight::clear(&self.transport);
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
        // The limits bound the picture, so a smaller scale lets a tall page,
        // such as a phone emulation laid out about 980 px wide, fit.
        let (scaled_width, scaled_height) =
            crate::acp::browser::admit_capture_pixels(f64::from(width), f64::from(height), scale)?;
        if scaled_width > f64::from(BROWSER_LIMITS.screenshot_width)
            || scaled_height > f64::from(BROWSER_LIMITS.screenshot_height)
        {
            return Err(BrowserFailure::Capacity);
        }
        let mut data = self.screenshot(format, (x, y, width, height), scale)?;
        // A PNG comes out in device pixels, which an emulated scale factor
        // of 2 to 3.5 multiplies; draw it once more at the scale that fits.
        if format == "png"
            && let Some(fit) = png_size(&data).and_then(|(w, h)| fit_factor(w, h))
        {
            data = self.screenshot(format, (x, y, width, height), scale * fit)?;
        }
        let data = data.as_str();
        if format == "png" {
            let (actual_width, actual_height) = admit_png(data)?;
            Ok((actual_width, actual_height, data.to_owned()))
        } else {
            admit_jpeg(width, height, data)?;
            Ok((width, height, data.to_owned()))
        }
    }

    fn screenshot(
        &mut self,
        format: &'static str,
        (x, y, width, height): (u32, u32, u32, u32),
        scale: f64,
    ) -> Result<String, BrowserFailure> {
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
        let mut response: Value =
            serde_json::from_str(&response).map_err(|_| BrowserFailure::Unavailable)?;
        match response["data"].take() {
            Value::String(data) => Ok(data),
            _ => Err(BrowserFailure::Unavailable),
        }
    }
}

/// Releases developer mode's object group without the dispatch permit:
/// after every evaluation and when a grant ends (ADR-0009b).
pub(in crate::browser::devtools) fn release_developer_objects<T: CdpTransport>(transport: &T) {
    let _ = transport.observe(
        "Runtime.releaseObjectGroup",
        &crate::acp::browser::developer::release_params(),
        4096,
        Duration::from_secs(2),
    );
}

impl<T: CdpTransport> DevtoolsPort for CdpPort<T> {
    fn raw(&mut self, method: &str, params: &str) -> Result<String, BrowserFailure> {
        let deadline = Instant::now() + Duration::from_secs(30);
        if method == "Runtime.evaluate" {
            return self.bounded_evaluate(params, deadline);
        }
        let bounded;
        let params = if method == "Page.captureScreenshot" {
            bounded = self.bounded_screenshot(params, deadline)?;
            bounded.as_str()
        } else {
            params
        };
        self.transport.raw(
            method,
            params,
            BROWSER_LIMITS.cdp_result_bytes,
            world::left(deadline)?,
        )
    }

    fn release_developer_objects(&mut self) {
        self.transport.release_raw_objects();
    }

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

    fn stopped(&self) -> bool {
        self.transport.stopped()
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
        // While the page loads it has no stamp. Only the incoming document
        // counts: never the outgoing one, nor a document already fully loaded
        // that the stamp has yet to report.
        let navigation = match self.stamp() {
            Ok(stamp) => Some(stamp.navigation_id),
            Err(BrowserFailure::Unavailable) => None,
            Err(error) => return Err(error),
        };
        let found = format!("document.body?.innerText?.includes({needle}) === true");
        let expression = match navigation {
            Some(_) => found,
            None => format!("document.readyState !== 'complete' && {found}"),
        };
        // A probe near the end of a wait, or one that makes the world, still
        // gets time to answer; the wait overruns its budget by at most this.
        let deadline = Instant::now() + remaining.max(PROBE_FLOOR);
        match self.evaluate_in_agent_world(navigation, &expression, 4096, deadline) {
            Ok(value) => Ok(value.as_bool().unwrap_or(false)),
            // The page navigated while the world was made, or still shows the
            // outgoing document; probe again.
            Err(BrowserFailure::StaleHandle) => Ok(false),
            // Chromium holds commands for a frame until its pending
            // navigation commits, so a probe can outlast its budget; the
            // probe is a permitless read, so the next one tries again.
            Err(BrowserFailure::OutcomeUnknown) => Ok(false),
            // A loading page can refuse a step for a moment.
            Err(BrowserFailure::Unavailable) if navigation.is_none() => Ok(false),
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
