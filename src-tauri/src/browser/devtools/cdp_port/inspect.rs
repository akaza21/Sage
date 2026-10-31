//! Legacy Chromium inspect primitives retained for the CDP lane, plus the
//! production cleanup that leaves no native highlight behind. Annotate
//! now tracks trusted hover and drag input locally in the isolated world.

#[cfg(test)]
use serde_json::{Value, json};
use std::time::Duration;

use crate::acp::browser::BrowserFailure;

use super::{CdpPort, CdpTransport};

const STEP_TIMEOUT: Duration = Duration::from_secs(2);
const RESULT_BYTES: usize = 4096;
/// A box model is six quads and two sizes; a shape-outside adds a path.
#[cfg(test)]
const BOX_MODEL_BYTES: usize = 16_384;

/// The Annotate blue (`--chat-annotation-accent`, #3a83f7).
#[cfg(test)]
const BLUE: (u8, u8, u8) = (58, 131, 247);

/// Removes the hover outline.
pub(crate) const HIDE_OUTLINE: (&str, &str) = ("Overlay.setShowHinge", "{}");

/// Steps that leave inspect mode, in order. Each runs even if one before it
/// failed, so the page never keeps a half-enabled overlay. The outline goes
/// one round trip after hovering ends, so an outline still on its way from
/// the last hover lands before it.
const STOP: [(&str, &str); 6] = [
    // Trusted-input Annotate may start before any Agent enabled DOM. Chromium
    // rejects DOM.disable in that state, so make cleanup safe on a fresh page
    // and on repeated starts while still surfacing actual transport failures.
    ("DOM.enable", "{}"),
    (
        "Overlay.setInspectMode",
        r#"{"mode":"none","highlightConfig":{}}"#,
    ),
    ("Overlay.hideHighlight", "{}"),
    HIDE_OUTLINE,
    ("Overlay.disable", "{}"),
    ("DOM.disable", "{}"),
];

impl<T: CdpTransport> CdpPort<T> {
    /// Turns on hover highlighting. The DOM agent gets the document node
    /// first, without which Chromium cannot name the hovered element. A
    /// failed step leaves inspect mode again.
    #[cfg(test)]
    pub(crate) fn start_inspect(&self) -> Result<(), BrowserFailure> {
        let started = [
            ("DOM.enable", "{}"),
            ("Overlay.enable", "{}"),
            ("DOM.getDocument", r#"{"depth":0}"#),
        ]
        .into_iter()
        .try_for_each(|(method, params)| self.inspect_step(method, params))
        .and_then(|()| self.inspect_step("Overlay.setInspectMode", &inspect_params()));
        if started.is_err() {
            let _ = self.stop_inspect();
        }
        started
    }

    /// Leaves inspect mode and clears any highlight; reports the first
    /// failure after trying every step.
    pub(crate) fn stop_inspect(&self) -> Result<(), BrowserFailure> {
        STOP.into_iter()
            .map(|(method, params)| self.inspect_step(method, params))
            .fold(Ok(()), Result::and)
    }

    /// Outlines the element an `Overlay.nodeHighlightRequested` event named,
    /// or removes the outline when that element has no box. WebView2 runs
    /// the same two steps from completion handlers in `devtools/picks.rs`,
    /// so as not to block its main thread; this blocking form drives tests.
    #[cfg(test)]
    pub(crate) fn outline_hovered(&self, node_id: u64) -> Result<(), BrowserFailure> {
        let outline = self
            .transport
            .observe(
                "DOM.getBoxModel",
                &box_model_params(node_id),
                BOX_MODEL_BYTES,
                STEP_TIMEOUT,
            )
            .ok()
            .and_then(|model| outline_params(&model, 1.0));
        self.inspect_step(HIDE_OUTLINE.0, outline.as_deref().unwrap_or(HIDE_OUTLINE.1))
    }

    fn inspect_step(&self, method: &str, params: &str) -> Result<(), BrowserFailure> {
        self.transport
            .observe(method, params, RESULT_BYTES, STEP_TIMEOUT)
            .map(drop)
    }
}

#[cfg(test)]
fn rgba((r, g, b): (u8, u8, u8), a: f64) -> Value {
    json!({"r": r, "g": g, "b": b, "a": a})
}

/// `searchForNode` drawing nothing itself: no box fills, info tooltip,
/// rulers or styles. It still takes the click and names the hovered element.
#[cfg(test)]
pub(super) fn inspect_params() -> String {
    let clear = rgba((0, 0, 0), 0.0);
    json!({
        "mode": "searchForNode",
        "highlightConfig": {
            "showInfo": false,
            "showStyles": false,
            "showRulers": false,
            "showAccessibilityInfo": false,
            "showExtensionLines": false,
            "contentColor": clear,
            "paddingColor": clear,
            "borderColor": clear,
            "marginColor": clear
        }
    })
    .to_string()
}

/// The element an `Overlay.nodeHighlightRequested` event names.
#[cfg(test)]
pub(crate) fn hovered_node(payload: &str) -> Option<u64> {
    if payload.len() > RESULT_BYTES {
        return None;
    }
    serde_json::from_str::<Value>(payload).ok()?["nodeId"]
        .as_u64()
        .filter(|id| *id > 0)
}

#[cfg(test)]
pub(crate) fn box_model_params(node_id: u64) -> String {
    json!({"nodeId": node_id}).to_string()
}

/// The hinge outlining a `DOM.getBoxModel` result's border box; `None` for
/// a missing, empty or malformed box. The box is in CSS px, while the hinge
/// is drawn in unzoomed px, so the rectangle is scaled by the page `zoom`
/// (1.0 at 100%). An inline element over several lines gets its bounding
/// rectangle.
#[cfg(test)]
pub(crate) fn outline_params(box_model: &str, zoom: f64) -> Option<String> {
    if box_model.len() > BOX_MODEL_BYTES || !zoom.is_finite() || !(0.25..=5.0).contains(&zoom) {
        return None;
    }
    let model = serde_json::from_str::<Value>(box_model).ok()?;
    let points = model["model"]["border"]
        .as_array()?
        .iter()
        .map(Value::as_f64)
        .collect::<Option<Vec<_>>>()
        .filter(|points| points.len() == 8 && points.iter().all(|point| point.is_finite()))?;
    let span = |axis: usize| {
        points
            .iter()
            .skip(axis)
            .step_by(2)
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), &point| {
                (low.min(point), high.max(point))
            })
    };
    let ((left, right), (top, bottom)) = (span(0), span(1));
    if right <= left || bottom <= top {
        return None;
    }
    Some(
        json!({"hingeConfig": {
            "rect": {
                "x": left * zoom,
                "y": top * zoom,
                "width": (right - left) * zoom,
                "height": (bottom - top) * zoom
            },
            "contentColor": rgba((0, 0, 0), 0.0),
            "outlineColor": rgba(BLUE, 1.0)
        }})
        .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    };

    use super::*;
    use crate::browser::devtools::{snapshot::DocumentStamp, transport::Settled};

    const BOX_MODEL: &str = r#"{"model":{"content":[12,22,108,22,108,48,12,48],"border":[10,20,110,20,110,50,10,50],"width":100,"height":30}}"#;

    #[derive(Default)]
    struct Recorder {
        calls: Mutex<Vec<(String, String, bool)>>,
        fail: Option<&'static str>,
        box_model: Option<&'static str>,
        dom_enabled: AtomicBool,
    }

    impl Recorder {
        fn methods(&self) -> Vec<String> {
            let calls = self.calls.lock().unwrap();
            calls.iter().map(|(method, _, _)| method.clone()).collect()
        }
        fn record(
            &self,
            method: &str,
            params: &str,
            permitted: bool,
        ) -> Result<String, BrowserFailure> {
            self.calls
                .lock()
                .unwrap()
                .push((method.into(), params.into(), permitted));
            if self.fail == Some(method) {
                return Err(BrowserFailure::Unavailable);
            }
            match method {
                "DOM.enable" => self.dom_enabled.store(true, Ordering::Relaxed),
                "DOM.disable" if !self.dom_enabled.swap(false, Ordering::Relaxed) => {
                    return Err(BrowserFailure::Unavailable);
                }
                _ => {}
            }
            Ok(match (method, self.box_model) {
                ("DOM.getBoxModel", Some(model)) => model.into(),
                _ => "{}".into(),
            })
        }
    }

    impl CdpTransport for Recorder {
        fn call(
            &self,
            method: &str,
            params: &str,
            _: usize,
            _: Duration,
        ) -> Result<String, BrowserFailure> {
            self.record(method, params, true)
        }
        fn observe(
            &self,
            method: &str,
            params: &str,
            _: usize,
            _: Duration,
        ) -> Result<String, BrowserFailure> {
            self.record(method, params, false)
        }
        fn stamp(&self) -> Result<DocumentStamp, BrowserFailure> {
            Err(BrowserFailure::Unavailable)
        }
        fn issued(&self) -> bool {
            false
        }
        fn settled(&self, _: DocumentStamp) -> Result<Option<Settled>, BrowserFailure> {
            Ok(None)
        }
    }

    fn port(fail: Option<&'static str>) -> CdpPort<Recorder> {
        CdpPort::new(Recorder {
            fail,
            ..Recorder::default()
        })
    }

    #[test]
    fn start_hands_the_document_over_then_searches_for_nodes_without_the_agent_permit() {
        let port = port(None);
        assert_eq!(port.start_inspect(), Ok(()));
        assert_eq!(
            port.transport.methods(),
            [
                "DOM.enable",
                "Overlay.enable",
                "DOM.getDocument",
                "Overlay.setInspectMode"
            ]
        );
        let calls = port.transport.calls.lock().unwrap();
        assert!(calls.iter().all(|(_, _, permitted)| !permitted));
        assert_eq!(calls[2].1, r#"{"depth":0}"#);
        let params: Value = serde_json::from_str(&calls[3].1).unwrap();
        assert_eq!(params["mode"], "searchForNode");
        let config = &params["highlightConfig"];
        for flag in [
            "showInfo",
            "showStyles",
            "showRulers",
            "showAccessibilityInfo",
            "showExtensionLines",
        ] {
            assert_eq!(config[flag], false, "{flag}");
        }
        // The inspect highlight tints nothing; the outline is Sage's.
        for fill in ["contentColor", "paddingColor", "borderColor", "marginColor"] {
            assert_eq!(config[fill]["a"], 0.0, "{fill}");
        }
    }

    #[test]
    fn a_failed_start_step_leaves_inspect_mode_again() {
        let port = port(Some("Overlay.setInspectMode"));
        assert_eq!(port.start_inspect(), Err(BrowserFailure::Unavailable));
        let methods = port.transport.methods();
        assert_eq!(methods[3], "Overlay.setInspectMode");
        assert_eq!(&methods[4..], STOP.map(|(method, _)| method));
    }

    #[test]
    fn stop_tries_every_step_and_reports_the_first_failure() {
        let port = port(Some("Overlay.hideHighlight"));
        assert_eq!(port.stop_inspect(), Err(BrowserFailure::Unavailable));
        assert_eq!(port.transport.methods(), STOP.map(|(method, _)| method));
        let calls = port.transport.calls.lock().unwrap();
        assert_eq!(calls[1].1, r#"{"mode":"none","highlightConfig":{}}"#);
        assert!(calls.iter().all(|(_, _, permitted)| !permitted));
    }

    #[test]
    fn stop_removes_the_outline_after_a_round_trip_and_before_the_overlay_goes() {
        let position = |step| STOP.iter().position(|entry| *entry == step).unwrap();
        assert_eq!(position(HIDE_OUTLINE), 3);
        assert!(position(("Overlay.disable", "{}")) > position(HIDE_OUTLINE));
    }

    #[test]
    fn cleanup_succeeds_on_a_fresh_page_and_when_repeated() {
        let port = port(None);
        assert_eq!(port.stop_inspect(), Ok(()));
        assert_eq!(port.stop_inspect(), Ok(()));
        assert!(!port.transport.dom_enabled.load(Ordering::Relaxed));
    }

    #[test]
    fn a_hovered_element_gets_a_blue_outline_around_its_border_box() {
        let port = CdpPort::new(Recorder {
            box_model: Some(BOX_MODEL),
            ..Recorder::default()
        });
        assert_eq!(port.outline_hovered(7), Ok(()));
        let calls = port.transport.calls.lock().unwrap();
        assert_eq!(calls[0].0, "DOM.getBoxModel");
        assert_eq!(calls[0].1, r#"{"nodeId":7}"#);
        assert_eq!(calls[1].0, "Overlay.setShowHinge");
        let hinge: Value = serde_json::from_str(&calls[1].1).unwrap();
        assert_eq!(
            hinge["hingeConfig"]["rect"],
            json!({"x": 10.0, "y": 20.0, "width": 100.0, "height": 30.0})
        );
        assert_eq!(
            hinge["hingeConfig"]["outlineColor"],
            json!({"r": 58, "g": 131, "b": 247, "a": 1.0})
        );
        assert_eq!(hinge["hingeConfig"]["contentColor"]["a"], 0.0);
        assert!(calls.iter().all(|(_, _, permitted)| !permitted));
    }

    #[test]
    fn an_element_without_a_box_removes_the_outline() {
        let port = port(Some("DOM.getBoxModel"));
        assert_eq!(port.outline_hovered(7), Ok(()));
        let calls = port.transport.calls.lock().unwrap();
        assert_eq!((calls[1].0.as_str(), calls[1].1.as_str()), HIDE_OUTLINE);
    }

    #[test]
    fn outline_params_bound_the_border_quad_and_refuse_bad_boxes() {
        // A transformed box gets the rectangle around its quad.
        let skewed = r#"{"model":{"border":[10,20,110,25,105,60,5,55]}}"#;
        let hinge: Value = serde_json::from_str(&outline_params(skewed, 1.0).unwrap()).unwrap();
        assert_eq!(
            hinge["hingeConfig"]["rect"],
            json!({"x": 5.0, "y": 20.0, "width": 105.0, "height": 40.0})
        );
        for bad in [
            "",
            "{}",
            r#"{"model":{"border":[0,0,0,0,0,0,0,0]}}"#,
            r#"{"model":{"border":[1,2,3]}}"#,
            r#"{"model":{"border":[1,2,3,4,5,6,7,"8"]}}"#,
        ] {
            assert_eq!(outline_params(bad, 1.0), None, "{bad}");
        }
        let huge = format!(r#"{{"pad":"{}"}}"#, "x".repeat(BOX_MODEL_BYTES));
        assert_eq!(outline_params(&huge, 1.0), None);
        for zoom in [0.0, -1.0, f64::NAN, f64::INFINITY, 9.0] {
            assert_eq!(outline_params(skewed, zoom), None, "{zoom}");
        }
    }

    #[test]
    fn outline_params_scale_the_css_box_by_the_page_zoom() {
        // At 150% page zoom a CSS-px box is drawn half again as far and as large.
        let hinge: Value = serde_json::from_str(&outline_params(BOX_MODEL, 1.5).unwrap()).unwrap();
        assert_eq!(
            hinge["hingeConfig"]["rect"],
            json!({"x": 15.0, "y": 30.0, "width": 150.0, "height": 45.0})
        );
    }

    #[test]
    fn hovered_node_reads_a_positive_node_id() {
        assert_eq!(hovered_node(r#"{"nodeId":42}"#), Some(42));
        assert_eq!(hovered_node(r#"{"nodeId":0}"#), None);
        assert_eq!(hovered_node(r#"{"backendNodeId":42}"#), None);
        assert_eq!(hovered_node("not json"), None);
        let huge = format!(r#"{{"nodeId":1,"pad":"{}"}}"#, "x".repeat(RESULT_BYTES));
        assert_eq!(hovered_node(&huge), None);
    }
}
