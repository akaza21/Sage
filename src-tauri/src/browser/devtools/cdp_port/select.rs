//! Annotate's pick (M1b): the element the user clicked in inspect mode is read
//! in the isolated Agent world and the accessibility tree, never the page's
//! main world, and the viewport is captured for the chrome to draw over.
//! Every step is a permitless read outside any Agent dispatch.

use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::{
    acp::browser::BrowserFailure,
    browser::annotate::{element::PageElement, selection::Frame},
};

use super::{CdpPort, CdpTransport, inspect::HIDE_OUTLINE, world::left};

const READ_TIMEOUT: Duration = Duration::from_secs(5);
const ELEMENT_BYTES: usize = 65_536;
/// The frozen frame's largest size.
const FRAME_BYTES: usize = 8_000_000;

/// Runs on the clicked element in the isolated world. A step is a tag, or
/// `:nth-child(k of tag)` among siblings of that tag: `tag:pseudo` would read
/// as a URL scheme to `redact_console` and fail the send-time check. It never
/// reads a form control's value: text inside inputs, text areas, selects and
/// editable regions is skipped, and `controls` says when the accessible name
/// may embed such a value (a control inside the element or its labels). Text
/// the user cannot see is skipped too: aria-hidden, invisible, transparent,
/// under 2px, or outside both the visible viewport and the element's box.
/// Password, hidden, card and one-time-code fields are `sensitive`. The
/// viewport is the visual one, so a pinch-zoomed page frames what it shows.
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
  const editable = '[contenteditable]:not([contenteditable="false"])';
  const control = `input, textarea, select, ${editable}`;
  const skip = `${control}, option, script, style, noscript, template, [aria-hidden="true"]`;
  const labels = [...(root.labels || []), ...(root.getAttribute('aria-labelledby') || '').split(/\s+/).map((id) => id && document.getElementById(id))];
  const controls = root.matches(editable) || !!root.querySelector(control)
    || labels.some((label) => label && [label, ...label.querySelectorAll(control)].some((node) => node !== root && node.matches(control)));
  const vv = visualViewport;
  const seen = { left: vv.offsetLeft, top: vv.offsetTop, right: vv.offsetLeft + vv.width, bottom: vv.offsetTop + vv.height };
  const rect = root.getBoundingClientRect();
  const meets = (a, b) => a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom;
  const alpha = (color) => color === 'transparent' ? 0 : parseFloat((color.match(/^rgba\(.*,\s*([\d.]+)\)$|\/\s*([\d.]+)\)$/) || []).slice(1).find(Boolean) ?? '1');
  const range = document.createRange();
  const pieces = [];
  let length = 0;
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  for (let node = walker.nextNode(); node && length < 2048; node = walker.nextNode()) {
    const parent = node.parentElement;
    if (!parent || parent.closest(skip)) continue;
    if (!parent.checkVisibility({ visibilityProperty: true, opacityProperty: true, contentVisibilityAuto: true })) continue;
    const style = getComputedStyle(parent);
    if (parseFloat(style.fontSize) < 2 || alpha(style.color) < 0.1 || alpha(style.webkitTextFillColor) < 0.1) continue;
    range.selectNodeContents(node);
    const box = range.getBoundingClientRect();
    if (box.width === 0 || box.height === 0 || !meets(box, seen) || !meets(box, rect)) continue;
    const text = node.data.replace(/\s+/g, ' ').trim();
    if (text) { pieces.push(text); length += text.length + 1; }
  }
  const autocomplete = (root.getAttribute('autocomplete') || '').toLowerCase();
  const sensitive = (root instanceof HTMLInputElement && (root.type === 'password' || root.type === 'hidden'))
    || /(^|\s)(cc-|one-time-code|current-password|new-password)/.test(autocomplete);
  return {
    selector: anchored.join(' > '),
    positional: positional.join(' > '),
    text: pieces.join(' ').slice(0, 2048),
    sensitive,
    controls,
    title: document.title.slice(0, 1024),
    rect: { x: rect.left + scrollX, y: rect.top + scrollY, width: rect.width, height: rect.height },
    viewport: { x: vv.pageLeft, y: vv.pageTop, width: innerWidth / vv.scale, height: innerHeight / vv.scale, clientWidth: vv.width, clientHeight: vv.height, scale: devicePixelRatio * vv.scale },
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
    /// The native Annotate owner reinstalls trusted input if a step fails.
    pub(crate) fn pick_element(&self, backend_node_id: u64) -> Result<PickedNode, BrowserFailure> {
        let before = self.stamp()?;
        let picked = self.read_element(backend_node_id, before.navigation_id);
        picked.and_then(|(page, role, name)| {
            self.pause_inspect()?;
            let frame = self.capture_frame()?;
            if self.stamp()? != before {
                return Err(BrowserFailure::StaleHandle);
            }
            Ok(PickedNode {
                page,
                role,
                name,
                frame,
            })
        })
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
    /// overlay stay enabled so `start_inspect` resumes it. The outline goes
    /// one round trip after hovering ends, as in `stop_inspect`, so the
    /// frozen frame never shows it.
    fn pause_inspect(&self) -> Result<(), BrowserFailure> {
        let deadline = Instant::now() + READ_TIMEOUT;
        self.read(
            "Overlay.setInspectMode",
            json!({"mode": "none", "highlightConfig": {}}),
            deadline,
        )?;
        self.read("Overlay.hideHighlight", json!({}), deadline)?;
        self.read(HIDE_OUTLINE.0, json!({}), deadline).map(drop)
    }

    /// What the webview shows, as a JPEG at device resolution: no clip, so
    /// no CSS px to device px conversion can apply page zoom, display
    /// scaling or pinch zoom twice. A larger image is retried at lower
    /// quality.
    pub(super) fn capture_frame(&self) -> Result<Frame, BrowserFailure> {
        let deadline = Instant::now() + READ_TIMEOUT;
        for quality in [85, 60, 40] {
            let params = json!({
                "format": "jpeg",
                "quality": quality,
                "fromSurface": true,
                "captureBeyondViewport": false,
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
                .filter(|data| data.len() <= FRAME_BYTES)
                .ok_or(BrowserFailure::Unavailable)?;
            let (width, height) = jpeg_size(data).ok_or(BrowserFailure::Unavailable)?;
            return Ok(Frame {
                jpeg_base64: data.to_owned(),
                png_base64: None,
                width,
                height,
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
#[cfg(test)]
pub(crate) fn inspected_node(payload: &str) -> Option<u64> {
    if payload.len() > 4096 {
        return None;
    }
    serde_json::from_str::<Value>(payload).ok()?["backendNodeId"]
        .as_u64()
        .filter(|id| *id > 0)
}

/// The width and height in a base64 JPEG's frame header, if it is one.
fn jpeg_size(data: &str) -> Option<(u32, u32)> {
    use base64::{Engine, engine::general_purpose::STANDARD};
    let bytes = STANDARD.decode(data).ok()?;
    if !bytes.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    let mut at = 2;
    loop {
        let (&[0xFF, marker], rest) = bytes.get(at..)?.split_first_chunk::<2>()? else {
            return None;
        };
        let length = usize::from(u16::from_be_bytes(*rest.first_chunk::<2>()?));
        // SOF0 to SOF15, leaving out DHT (C4), JPG (C8) and DAC (CC).
        if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            let header = rest.get(3..7)?;
            let side = |at: usize| u32::from(u16::from_be_bytes([header[at], header[at + 1]]));
            return Some((side(2), side(0))).filter(|(width, height)| *width > 0 && *height > 0);
        }
        at += 2 + length;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn the_frame_size_is_read_from_the_jpeg_header() {
        use base64::{Engine, engine::general_purpose::STANDARD};
        // SOI, an APP0 segment to skip, then SOF0 for 780x436.
        let jpeg = [
            0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x4A, 0x46, 0xFF, 0xC0, 0x00, 0x11, 0x08, 0x01,
            0xB4, 0x03, 0x0C, 0x03,
        ];
        assert_eq!(jpeg_size(&STANDARD.encode(jpeg)), Some((780, 436)));
        assert_eq!(jpeg_size(&STANDARD.encode(&jpeg[..12])), None);
        assert_eq!(jpeg_size(&STANDARD.encode([0x89, b'P', b'N', b'G'])), None);
        assert_eq!(jpeg_size("/9j/<img"), None);
    }
}
