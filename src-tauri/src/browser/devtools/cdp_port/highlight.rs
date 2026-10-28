//! The Agent's act marker, drawn by Chromium's overlay above the page: the
//! target element is outlined and a small cursor dot sits at its centre. The
//! page's main world runs nothing. The overlay keeps one highlight tool, so
//! the dot is the overlay's separate hinge rectangle, which Chromium draws
//! alongside a node highlight and which does not change the page's viewport
//! segments. Annotate shares the same overlay (BP-M1a-b).

use serde_json::{Value, json};
use std::time::Duration;

use super::{CdpPort, CdpTransport};
use crate::browser::devtools::snapshot::DocumentStamp;

const BLUE: (u8, u8, u8) = (74, 143, 255);
const CURSOR_PX: f64 = 10.0;

/// What clears the marker: on Stop, Take over, detach and navigation start.
pub(in crate::browser::devtools) const CLEAR: [(&str, &str); 2] = [
    ("Overlay.hideHighlight", "{}"),
    ("Overlay.setShowHinge", "{}"),
];

pub(super) fn mark<T: CdpTransport>(port: &CdpPort<T>, stamp: DocumentStamp, backend_id: u64) {
    if port.stamp().ok() != Some(stamp) {
        return;
    }
    let call =
        |method: &str, params: String| port.call(method, &params, 4096, Duration::from_secs(1));
    // Chromium refuses `Overlay.enable` until the DOM domain is on.
    if call("DOM.enable", "{}".into()).is_err()
        || call("Overlay.enable", "{}".into()).is_err()
        || call("Overlay.highlightNode", highlight_params(backend_id)).is_err()
    {
        return;
    }
    let centre = call(
        "DOM.getBoxModel",
        json!({"backendNodeId": backend_id}).to_string(),
    )
    .ok()
    .and_then(|model| border_centre(&model));
    if let Some((x, y)) = centre {
        let _ = call("Overlay.setShowHinge", cursor_params(x, y));
    }
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
