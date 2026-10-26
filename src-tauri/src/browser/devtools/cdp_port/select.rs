//! Annotate's pick (M1b): the element the user clicked in inspect mode is read
//! in the isolated Agent world and the accessibility tree, never the page's
//! main world, and the viewport is captured for the chrome to draw over.
//! Every step is a permitless read outside any Agent dispatch.

use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::{
    acp::browser::BrowserFailure,
    browser::annotate::{
        element::{PageElement, Viewport},
        selection::Frame,
    },
};

use super::{CdpPort, CdpTransport, world::left};

const READ_TIMEOUT: Duration = Duration::from_secs(5);
const ELEMENT_BYTES: usize = 65_536;
/// The frozen frame's largest side in device pixels, and its largest size.
const FRAME_SIDE: f64 = 4096.0;
const FRAME_BYTES: usize = 8_000_000;

/// Runs on the clicked element in the isolated world. A step is a tag, or
/// `:nth-child(k of tag)` among siblings of that tag: `tag:pseudo` would read
/// as a URL scheme to `redact_console` and fail the send-time check. It never
/// reads a form
/// control's value: text inside inputs, text areas, selects and editable
/// regions is skipped, as is text that is aria-hidden, invisible or has no
/// size. Password, hidden, card and one-time-code fields are `sensitive`.
const READ_ELEMENT: &str = r#"function () {
  const root = this;
  if (!(root instanceof Element)) return null;
  const step = (node) => {
    const tag = CSS.escape(node.localName);
    if (!node.parentElement) return tag;
    const same = [...node.parentElement.children].filter((sibling) => sibling.localName === node.localName);
    return same.length === 1 ? tag : `:nth-child(${same.indexOf(node) + 1} of ${tag})`;
  };
  const anchored = [], positional = [];
  let anchor = false;
  for (let node = root; node && positional.length < 16; node = node.parentElement) {
    positional.unshift(step(node));
    if (anchor) continue;
    if (/^[A-Za-z][\w-]{0,63}$/.test(node.id) && document.querySelectorAll(`#${node.id}`).length === 1) {
      anchored.unshift(`#${node.id}`);
      anchor = true;
    } else anchored.unshift(step(node));
  }
  const skip = 'input, textarea, select, option, script, style, noscript, template, [contenteditable]:not([contenteditable="false"]), [aria-hidden="true"]';
  const pieces = [];
  let length = 0;
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  for (let node = walker.nextNode(); node && length < 2048; node = walker.nextNode()) {
    const parent = node.parentElement;
    if (!parent || parent.closest(skip)) continue;
    if (!parent.checkVisibility({ visibilityProperty: true })) continue;
    const box = parent.getBoundingClientRect();
    if (box.width === 0 || box.height === 0) continue;
    const text = node.data.replace(/\s+/g, ' ').trim();
    if (text) { pieces.push(text); length += text.length + 1; }
  }
  const autocomplete = (root.getAttribute('autocomplete') || '').toLowerCase();
  const sensitive = (root instanceof HTMLInputElement && (root.type === 'password' || root.type === 'hidden'))
    || /(^|\s)(cc-|one-time-code|current-password|new-password)/.test(autocomplete);
  const rect = root.getBoundingClientRect();
  const page = document.documentElement;
  return {
    selector: anchored.join(' > '),
    positional: positional.join(' > '),
    text: pieces.join(' ').slice(0, 2048),
    sensitive,
    title: document.title.slice(0, 1024),
    rect: { x: rect.left + scrollX, y: rect.top + scrollY, width: rect.width, height: rect.height },
    viewport: { x: scrollX, y: scrollY, width: innerWidth, height: innerHeight, clientWidth: page.clientWidth, clientHeight: page.clientHeight, scale: devicePixelRatio },
  };
}"#;

/// The element the user picked, its accessible role and name, and the
/// frozen frame.
pub(crate) struct PickedNode {
    pub page: PageElement,
    pub role: String,
    pub name: String,
    pub frame: Frame,
}

impl<T: CdpTransport> CdpPort<T> {
    /// Reads the element behind `backend_node_id`, pauses hover highlighting
    /// and captures the viewport, only while the page keeps one document.
    /// Hover highlighting resumes if any step fails.
    pub(crate) fn pick_element(&self, backend_node_id: u64) -> Result<PickedNode, BrowserFailure> {
        let before = self.stamp()?;
        let picked = self.read_element(backend_node_id, before.navigation_id);
        let picked = picked.and_then(|(page, role, name)| {
            self.pause_inspect()?;
            let frame = self.capture_frame(&page.viewport)?;
            if self.stamp()? != before {
                return Err(BrowserFailure::StaleHandle);
            }
            Ok(PickedNode {
                page,
                role,
                name,
                frame,
            })
        });
        if picked.is_err() {
            let _ = self.start_inspect();
        }
        picked
    }

    fn read_element(
        &self,
        backend_node_id: u64,
        navigation: u64,
    ) -> Result<(PageElement, String, String), BrowserFailure> {
        let deadline = Instant::now() + READ_TIMEOUT;
        let value = self.in_agent_world(Some(navigation), true, deadline, |context, _| {
            let resolved = self.read(
                "DOM.resolveNode",
                json!({"backendNodeId": backend_node_id, "executionContextId": context}),
                deadline,
            )?;
            let object = resolved["object"]["objectId"]
                .as_str()
                .ok_or(BrowserFailure::StaleHandle)?;
            let result = self.read(
                "Runtime.callFunctionOn",
                json!({"objectId": object, "functionDeclaration": READ_ELEMENT, "returnByValue": true}),
                deadline,
            );
            let _ = self.read(
                "Runtime.releaseObject",
                json!({"objectId": object}),
                deadline,
            );
            result
        })?;
        if value.get("exceptionDetails").is_some() {
            return Err(BrowserFailure::Unavailable);
        }
        let page: PageElement = serde_json::from_value(value["result"]["value"].clone())
            .map_err(|_| BrowserFailure::StaleHandle)?;
        if !page.viewport.admitted() {
            return Err(BrowserFailure::Unavailable);
        }
        let tree = self.read(
            "Accessibility.getPartialAXTree",
            json!({"backendNodeId": backend_node_id, "fetchRelatives": false}),
            deadline,
        )?;
        let node = &tree["nodes"][0];
        let text = |field: &str| node[field]["value"].as_str().unwrap_or("").to_owned();
        Ok((page, text("role"), text("name")))
    }

    /// Leaves hover highlighting off while the user comments; DOM and the
    /// overlay stay enabled so `start_inspect` resumes it.
    fn pause_inspect(&self) -> Result<(), BrowserFailure> {
        let deadline = Instant::now() + READ_TIMEOUT;
        self.read(
            "Overlay.setInspectMode",
            json!({"mode": "none", "highlightConfig": {}}),
            deadline,
        )?;
        self.read("Overlay.hideHighlight", json!({}), deadline)
            .map(drop)
    }

    /// The viewport as a JPEG at device resolution, smaller when it would
    /// exceed the frame limits.
    fn capture_frame(&self, viewport: &Viewport) -> Result<Frame, BrowserFailure> {
        let deadline = Instant::now() + READ_TIMEOUT;
        let scale = frame_scale(viewport);
        for factor in [1.0, 0.75, 0.5] {
            let scale = scale * factor;
            let params = json!({
                "format": "jpeg",
                "quality": 85,
                "fromSurface": true,
                "captureBeyondViewport": false,
                "clip": {
                    "x": viewport.x,
                    "y": viewport.y,
                    "width": viewport.client_width,
                    "height": viewport.client_height,
                    "scale": scale,
                },
            });
            let response = match self.transport.observe(
                "Page.captureScreenshot",
                &params.to_string(),
                FRAME_BYTES + 4096,
                left(deadline)?,
            ) {
                Err(BrowserFailure::Capacity) => continue,
                response => response?,
            };
            let response: Value =
                serde_json::from_str(&response).map_err(|_| BrowserFailure::Unavailable)?;
            let data = response["data"]
                .as_str()
                .filter(|data| data.len() <= FRAME_BYTES && jpeg_base64(data))
                .ok_or(BrowserFailure::Unavailable)?;
            return Ok(Frame {
                jpeg_base64: data.to_owned(),
                width: (viewport.client_width * scale).round() as u32,
                height: (viewport.client_height * scale).round() as u32,
            });
        }
        Err(BrowserFailure::Capacity)
    }

    fn read(
        &self,
        method: &str,
        params: Value,
        deadline: Instant,
    ) -> Result<Value, BrowserFailure> {
        let response =
            self.transport
                .observe(method, &params.to_string(), ELEMENT_BYTES, left(deadline)?)?;
        serde_json::from_str(&response).map_err(|_| BrowserFailure::Unavailable)
    }
}

/// The node an `Overlay.inspectNodeRequested` event names.
pub(crate) fn inspected_node(payload: &str) -> Option<u64> {
    if payload.len() > 4096 {
        return None;
    }
    serde_json::from_str::<Value>(payload).ok()?["backendNodeId"]
        .as_u64()
        .filter(|id| *id > 0)
}

/// Device pixels per CSS px for the frame, capped so neither side exceeds
/// `FRAME_SIDE`.
fn frame_scale(viewport: &Viewport) -> f64 {
    viewport
        .scale
        .min(FRAME_SIDE / viewport.client_width)
        .min(FRAME_SIDE / viewport.client_height)
}

fn jpeg_base64(data: &str) -> bool {
    data.starts_with("/9j/")
        && data.len().is_multiple_of(4)
        && data
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'='))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn viewport(width: f64, height: f64, scale: f64) -> Viewport {
        Viewport {
            x: 0.0,
            y: 0.0,
            width,
            height,
            client_width: width,
            client_height: height,
            scale,
        }
    }

    #[test]
    fn the_frame_keeps_device_resolution_within_its_largest_side() {
        assert_eq!(frame_scale(&viewport(1280.0, 720.0, 1.5)), 1.5);
        assert_eq!(frame_scale(&viewport(3000.0, 1000.0, 2.0)), 4096.0 / 3000.0);
        assert_eq!(frame_scale(&viewport(800.0, 5000.0, 1.0)), 4096.0 / 5000.0);
    }

    #[test]
    fn a_click_report_names_one_positive_backend_node() {
        assert_eq!(inspected_node(r#"{"backendNodeId":42}"#), Some(42));
        assert_eq!(inspected_node(r#"{"backendNodeId":0}"#), None);
        assert_eq!(inspected_node(r#"{"nodeId":42}"#), None);
        assert_eq!(inspected_node("not json"), None);
        let padded = format!(r#"{{"backendNodeId":42,"pad":"{}"}}"#, "x".repeat(4096));
        assert_eq!(inspected_node(&padded), None);
    }

    #[test]
    fn only_base64_jpeg_data_is_kept() {
        assert!(jpeg_base64("/9j/4AAQ"));
        assert!(!jpeg_base64("iVBORw0K"));
        assert!(!jpeg_base64("/9j/4AA"));
        assert!(!jpeg_base64("/9j/<img"));
    }
}
