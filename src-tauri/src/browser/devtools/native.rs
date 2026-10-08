use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tauri::{Manager, Webview};

use crate::acp::{
    browser::{
        BrowserAction, BrowserCapability, BrowserFailure, BrowserRequest, BrowserResult,
        BrowserScope, ViewportRegion, WaitCondition,
        permission::{BrowserCapabilityDispatch, BrowserDispatchPermit, PendingBrowserResult},
    },
    limits::BROWSER_LIMITS,
};

use super::{
    capability::{DevtoolsCapability, DevtoolsPort},
    cdp, events,
    screenshot::{admit_jpeg, admit_png, capture_png_with_fallback},
    snapshot::{DocumentStamp, NativeNode, parse_ax_tree},
};
use crate::browser::BrowserState;

mod act;
mod highlight;
mod outcome;
mod page_context;
pub(crate) use highlight::clear as clear_agent_highlight;
use outcome::{
    dispatch_disconnected_failure, dispatch_timeout_failure, navigation_failure, navigation_matches,
};
pub(crate) use page_context::capture_page_context;

pub(crate) fn capture_user_png(
    state: &BrowserState,
    tab_id: &str,
    expected_revision: &str,
) -> Result<(u32, u32, String), String> {
    let webview = state
        .tabs
        .lock()
        .map_err(|_| "browser unavailable")?
        .handle(tab_id)
        .cloned()
        .ok_or("browser tab unavailable")?;
    let mut port = NativePort {
        webview,
        permit: None,
        issued: Arc::new(AtomicBool::new(false)),
    };
    let before = port.stamp().map_err(|_| "browser page unavailable")?;
    if before.generation.to_string() != expected_revision {
        return Err("browser page changed; retry screenshot".into());
    }
    let captured = capture_png_with_fallback(|scale| port.capture_image(None, "png", scale))
        .map_err(|error| match error {
            BrowserFailure::Capacity => "browser screenshot exceeds size limit",
            _ => "browser screenshot unavailable",
        })?;
    if port
        .stamp()
        .map_err(|_| "browser page changed; retry screenshot")?
        != before
    {
        return Err("browser page changed; retry screenshot".into());
    }
    Ok(captured)
}

pub(crate) type NativeDevtools = DevtoolsCapability<NativePort>;

impl BrowserCapabilityDispatch for &mut NativeDevtools {
    fn dispatch(
        &mut self,
        request: &BrowserRequest,
        permit: BrowserDispatchPermit,
    ) -> Result<PendingBrowserResult, BrowserFailure> {
        let mut capability = (**self).clone();
        let timeout_permit = permit.clone();
        let issued = Arc::new(AtomicBool::new(false));
        capability.port_mut().issued = issued.clone();
        capability.port_mut().permit = Some(permit);
        let request = request.clone();
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let _ = sender.send(capability.perform(&request));
        });
        Ok(Box::new(move || {
            let deadline = Instant::now() + Duration::from_secs(35);
            loop {
                if !timeout_permit.is_live() && !timeout_permit.was_invalidated_by_navigation() {
                    return Err(timeout_permit.revoked_failure());
                }
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    timeout_permit.revoke();
                    return Err(dispatch_timeout_failure(issued.load(Ordering::Acquire)));
                }
                match receiver.recv_timeout(remaining.min(Duration::from_millis(25))) {
                    Ok(result) => return result,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                        return Err(dispatch_disconnected_failure(
                            issued.load(Ordering::Acquire),
                        ));
                    }
                }
            }
        }))
    }
}

#[derive(Clone)]
pub(crate) struct NativePort {
    webview: Webview,
    permit: Option<BrowserDispatchPermit>,
    issued: Arc<AtomicBool>,
}

impl NativePort {
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

    fn call(
        &self,
        method: &str,
        params: &str,
        max_result_bytes: usize,
        timeout: Duration,
    ) -> Result<String, BrowserFailure> {
        cdp::call_with_permit(
            &self.webview,
            method,
            params,
            max_result_bytes,
            timeout,
            self.permit.clone(),
            Some(self.issued.clone()),
        )
    }

    fn stamp(&self) -> Result<DocumentStamp, BrowserFailure> {
        let app = self.webview.app_handle();
        let state = app.state::<BrowserState>();
        let tabs = state.tabs.lock().map_err(|_| BrowserFailure::Unavailable)?;
        let tab = tabs
            .snapshot(self.webview.label())
            .ok_or(BrowserFailure::StaleHandle)?;
        if tab.loading || tab.error.is_some() || tab.committed_url.is_none() {
            return Err(BrowserFailure::Unavailable);
        }
        Ok(DocumentStamp {
            navigation_id: tab.navigation_id.unwrap_or(0),
            generation: tab.revision,
        })
    }
}

impl DevtoolsCapability<NativePort> {
    pub(crate) fn for_tab(
        state: &BrowserState,
        scope: BrowserScope,
    ) -> Result<Self, BrowserFailure> {
        let label = format!("browser-{}", scope.tab_id);
        let webview = state
            .tabs
            .lock()
            .map_err(|_| BrowserFailure::Unavailable)?
            .handle(&label)
            .cloned()
            .ok_or(BrowserFailure::Unavailable)?;
        Self::from_webview(scope, webview)
    }

    fn from_webview(scope: BrowserScope, webview: Webview) -> Result<Self, BrowserFailure> {
        if webview.label() != format!("browser-{}", scope.tab_id) {
            return Err(BrowserFailure::Denied);
        }
        let diagnostics = events::attach(&webview)?;
        Ok(Self::with_diagnostics(
            scope,
            NativePort {
                webview,
                permit: None,
                issued: Arc::new(AtomicBool::new(false)),
            },
            diagnostics,
        ))
    }
}

impl DevtoolsPort for NativePort {
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
            .map_err(|error| navigation_failure(self.issued.load(Ordering::Acquire), error))?;
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
            if self
                .permit
                .as_ref()
                .is_some_and(|permit| !permit.is_live() && !permit.was_invalidated_by_navigation())
            {
                return Err(self.permit.as_ref().unwrap().revoked_failure());
            }
            let state = self.webview.app_handle().state::<BrowserState>();
            let snapshot = state
                .tabs
                .lock()
                .map_err(|_| BrowserFailure::OutcomeUnknown)?
                .snapshot(self.webview.label())
                .ok_or(BrowserFailure::OutcomeUnknown)?;
            if snapshot.revision > before.generation && !snapshot.loading {
                if snapshot.error.is_some() {
                    return Err(BrowserFailure::OutcomeUnknown);
                }
                if let Some(committed_url) = snapshot.committed_url {
                    let settled_loader_id = if issued_loader_id.is_some() {
                        let tree = cdp::call(
                            &self.webview,
                            "Page.getFrameTree",
                            "{}",
                            4096,
                            Duration::from_secs(5),
                        )
                        .map_err(|_| BrowserFailure::OutcomeUnknown)?;
                        let tree: Value = serde_json::from_str(&tree)
                            .map_err(|_| BrowserFailure::OutcomeUnknown)?;
                        tree["frameTree"]["frame"]["loaderId"]
                            .as_str()
                            .map(str::to_owned)
                    } else {
                        None
                    };
                    if !navigation_matches(
                        url,
                        issued_loader_id.as_deref(),
                        &committed_url,
                        settled_loader_id.as_deref(),
                    ) {
                        return Err(BrowserFailure::OutcomeUnknown);
                    }
                    return Ok((committed_url, snapshot.navigation_id.unwrap_or(0)));
                }
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
        let expression = format!("document.body?.innerText?.includes({needle}) === true");
        let params = json!({"expression": expression, "returnByValue": true});
        let response = self.call("Runtime.evaluate", &params.to_string(), 4096, remaining)?;
        let response: Value =
            serde_json::from_str(&response).map_err(|_| BrowserFailure::Unavailable)?;
        Ok(response["result"]["value"].as_bool().unwrap_or(false))
    }
}

fn dimension(value: &Value) -> Result<u32, BrowserFailure> {
    let value = value.as_f64().ok_or(BrowserFailure::Unavailable)?;
    if !value.is_finite() || value <= 0.0 || value > f64::from(u32::MAX) {
        return Err(BrowserFailure::Capacity);
    }
    Ok(value.ceil() as u32)
}
