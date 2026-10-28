//! The Agent's act marker, drawn by Chromium's overlay above the page: the
//! target element is outlined and a small cursor dot sits at its centre. The
//! page's main world runs nothing. The overlay keeps one highlight tool, so
//! the dot is the overlay's separate hinge rectangle, which Chromium draws
//! alongside a node highlight and which does not change the page's viewport
//! segments. Annotate shares the same overlay (BP-M1a-b) and draws its hover
//! outline with that one hinge (BP-H2, `inspect::HIDE_OUTLINE`). So while the
//! tab annotates the marker is the outline alone: it neither draws nor
//! clears the hinge, and its clear leaves the overlay and DOM domains on.
//!
//! The marker shows for `LEAD` before the act and is cleared once the act
//! returns, so it is never left on the page, never scrolls out of place and
//! never reaches a capture; captures clear it first as well.

use serde_json::{Value, json};
use std::time::Duration;

use super::{CdpPort, CdpTransport};
use crate::browser::devtools::snapshot::DocumentStamp;

const BLUE: (u8, u8, u8) = (74, 143, 255);
const CURSOR_PX: f64 = 10.0;
/// Each marker step is a permitless read with this budget: a slow or lost
/// step never revokes the Agent's permit or fails the act it marks.
const STEP_TIMEOUT: Duration = Duration::from_secs(1);
/// How long the marker shows before the act runs, so the user sees the
/// target first and can still take over.
pub(super) const LEAD: Duration = Duration::from_millis(300);

/// What removes the marker: after each act and before each capture, and on
/// Stop, Take over, detach and navigation start.
pub(in crate::browser::devtools) const CLEAR: [(&str, &str); 2] = [
    ("Overlay.hideHighlight", "{}"),
    ("Overlay.setShowHinge", "{}"),
];

/// What turns the overlay off once the marker is gone, unless the tab
/// annotates: its inspect overlay needs both domains.
pub(in crate::browser::devtools) const DISABLE: [(&str, &str); 2] =
    [("Overlay.disable", "{}"), ("DOM.disable", "{}")];

/// Draws the marker; true when the outline is on the page. Best effort: it
/// sends nothing once the permit is stopped, and every step is permitless,
/// so it can neither revoke the permit nor fail the act.
pub(in crate::browser::devtools) fn mark<T: CdpTransport>(
    port: &CdpPort<T>,
    stamp: DocumentStamp,
    backend_id: u64,
) -> bool {
    if port.transport.stopped() || port.stamp().ok() != Some(stamp) {
        return false;
    }
    let step =
        |method: &str, params: String| port.transport.observe(method, &params, 4096, STEP_TIMEOUT);
    // Chromium refuses `Overlay.enable` until the DOM domain is on.
    if step("DOM.enable", "{}".into()).is_err()
        || step("Overlay.enable", "{}".into()).is_err()
        || step("Overlay.highlightNode", highlight_params(backend_id)).is_err()
    {
        clear(&port.transport);
        return false;
    }
    if port.transport.annotating() {
        return true;
    }
    let centre = step(
        "DOM.getBoxModel",
        json!({"backendNodeId": backend_id}).to_string(),
    )
    .ok()
    .and_then(|model| border_centre(&model));
    if let Some((x, y)) = centre {
        let _ = step("Overlay.setShowHinge", cursor_params(x, y));
    }
    true
}

/// Removes the marker and turns the overlay and DOM domains off again.
/// While this tab annotates the hinge is Annotate's hover outline, so only
/// the outline goes. The domains are turned off only when no Annotate
/// change is under way on any tab, since a start enables them; Annotate
/// busy elsewhere never leaves the cursor dot on this page. Every step runs
/// even if one before it failed.
pub(in crate::browser::devtools) fn clear<T: CdpTransport>(transport: &T) {
    let send = |(method, params): (&str, &str)| {
        let _ = transport.observe(method, params, 4096, STEP_TIMEOUT);
    };
    send(CLEAR[0]);
    if !transport.annotating() {
        send(CLEAR[1]);
    }
    transport.unless_annotating(&mut || DISABLE.into_iter().for_each(send));
}

fn rgba(alpha: f64) -> Value {
    json!({"r": BLUE.0, "g": BLUE.1, "b": BLUE.2, "a": alpha})
}

fn highlight_params(backend_id: u64) -> String {
    json!({
        "backendNodeId": backend_id,
        "highlightConfig": {
            "showInfo": false,
            "showStyles": false,
            "showRulers": false,
            "contentColor": rgba(0.24),
            "borderColor": rgba(0.9)
        }
    })
    .to_string()
}

fn cursor_params(x: f64, y: f64) -> String {
    json!({
        "hingeConfig": {
            "rect": {
                "x": (x - CURSOR_PX / 2.0).round(),
                "y": (y - CURSOR_PX / 2.0).round(),
                "width": CURSOR_PX,
                "height": CURSOR_PX
            },
            "contentColor": rgba(1.0),
            "outlineColor": {"r": 255, "g": 255, "b": 255, "a": 1.0}
        }
    })
    .to_string()
}

/// The centre of the border quad of a `DOM.getBoxModel` answer, in viewport
/// CSS pixels.
fn border_centre(model: &str) -> Option<(f64, f64)> {
    let model: Value = serde_json::from_str(model).ok()?;
    let quad = model["model"]["border"].as_array()?;
    if quad.len() != 8 {
        return None;
    }
    let point = |index: usize| quad[index].as_f64().filter(|value| value.is_finite());
    Some(((point(0)? + point(4)?) / 2.0, (point(1)? + point(5)?) / 2.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlight_uses_only_a_native_document_node_id() {
        let params: Value = serde_json::from_str(&highlight_params(42)).unwrap();
        assert_eq!(params["backendNodeId"], 42);
        assert_eq!(params["highlightConfig"]["showInfo"], false);
        assert!(!highlight_params(42).contains("value"));
    }

    #[test]
    fn the_cursor_dot_centres_on_the_element_border_box() {
        let model = json!({"model": {"border": [100, 100, 220, 100, 220, 140, 100, 140]}});
        let (x, y) = border_centre(&model.to_string()).unwrap();
        assert_eq!((x, y), (160.0, 120.0));
        let params: Value = serde_json::from_str(&cursor_params(x, y)).unwrap();
        assert_eq!(
            params["hingeConfig"]["rect"],
            json!({"x": 155.0, "y": 115.0, "width": 10.0, "height": 10.0})
        );
        assert!(border_centre(r#"{"model":{"border":[1,2]}}"#).is_none());
        assert!(border_centre("not json").is_none());
    }
}
