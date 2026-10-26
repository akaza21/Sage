//! Annotate mode (M1a): Chromium's own inspect overlay outlines the element
//! under the pointer, leaving inspect mode clears it, and the page's main
//! world sees neither.

use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::{CdpTransport, Lane};

const TIMEOUT: Duration = Duration::from_secs(5);

/// A test-side CDP call outside the Agent transcript.
pub(super) fn cdp(lane: &mut Lane, method: &str, params: Value) -> Value {
    let result = lane
        .capability
        .port_mut()
        .transport
        .observe(method, &params.to_string(), 4 * 1_048_576, TIMEOUT)
        .unwrap_or_else(|error| panic!("{method}: {error:?}"));
    serde_json::from_str(&result).expect("CDP result")
}

/// Counts every DOM mutation and fixes the global count from here on.
pub(super) fn watch_main_world(lane: &mut Lane) {
    lane.evaluate(
        "window.__mutations = 0; new MutationObserver((records) => { window.__mutations += records.length; }).observe(document, { subtree: true, childList: true, attributes: true, characterData: true }); window.__globalsAtLoad = Object.getOwnPropertyNames(window).length; true",
    );
}

pub(super) fn assert_main_world_untouched(lane: &mut Lane) {
    assert_eq!(lane.evaluate("window.__mutations"), json!(0));
    assert_eq!(
        lane.evaluate("Object.getOwnPropertyNames(window).length - window.__globalsAtLoad"),
        json!(0)
    );
    assert_eq!(lane.evaluate("window.__agentSeen"), json!([]));
}

/// The button's node ids and centre, read natively with DOM enabled.
pub(super) fn press_button(lane: &mut Lane) -> (u64, u64, f64, f64) {
    let root = cdp(lane, "DOM.getDocument", json!({"depth": 0}));
    let node = cdp(
        lane,
        "DOM.querySelector",
        json!({"nodeId": root["root"]["nodeId"], "selector": "#press"}),
    )["nodeId"]
        .as_u64()
        .expect("button node");
    let described = cdp(lane, "DOM.describeNode", json!({"nodeId": node}));
    let backend = described["node"]["backendNodeId"].as_u64().unwrap();
    let quad = cdp(lane, "DOM.getBoxModel", json!({"nodeId": node}))["model"]["border"].clone();
    let corner = |index: usize| quad[index].as_f64().unwrap();
    (
        node,
        backend,
        (corner(0) + corner(4)) / 2.0,
        (corner(1) + corner(5)) / 2.0,
    )
}

pub(super) fn mouse(lane: &mut Lane, kind: &str, x: f64, y: f64) {
    let mut params = json!({"type": kind, "x": x, "y": y});
    if kind != "mouseMoved" {
        params["button"] = "left".into();
        params["clickCount"] = 1.into();
    }
    cdp(lane, "Input.dispatchMouseEvent", params);
}

/// The viewport as PNG. A clipped capture renders a fresh frame without the
/// inspect overlay, so the whole viewport is compared.
fn capture(lane: &mut Lane) -> String {
    cdp(lane, "Page.captureScreenshot", json!({"format": "png"}))["data"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// Waits for an `Overlay.*` event named `method`; other events are dropped.
pub(super) fn overlay_event(lane: &mut Lane, method: &str) -> Option<Value> {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        let events = lane.capability.port_mut().transport.take_overlay_events();
        if let Some(event) = events.into_iter().find(|event| event["method"] == method) {
            return Some(event["params"].clone());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    None
}

#[test]
fn annotate_highlights_the_hovered_element_natively_and_leaving_clears_it() {
    let mut lane = Lane::open("/hostile.html");
    watch_main_world(&mut lane);
    lane.capability.port_mut().transport.take_overlay_events();
    let (_, _, x, y) = press_button(&mut lane);
    mouse(&mut lane, "mouseMoved", x, y);
    let plain = capture(&mut lane);

    lane.capability
        .port_mut()
        .start_inspect()
        .expect("start inspect mode");
    let (node, backend, x, y) = press_button(&mut lane);
    mouse(&mut lane, "mouseMoved", x + 1.0, y);
    let hovered = overlay_event(&mut lane, "Overlay.nodeHighlightRequested")
        .expect("hover asks for a highlight");
    assert_eq!(hovered["nodeId"].as_u64(), Some(node));
    assert_ne!(
        capture(&mut lane),
        plain,
        "the inspect overlay draws over the hovered button"
    );

    // Inspect mode takes the click: Chromium reports the node and the page
    // never sees it.
    mouse(&mut lane, "mousePressed", x + 1.0, y);
    mouse(&mut lane, "mouseReleased", x + 1.0, y);
    let clicked = overlay_event(&mut lane, "Overlay.inspectNodeRequested")
        .expect("a click in inspect mode names the node");
    assert_eq!(clicked["backendNodeId"].as_u64(), Some(backend));
    assert!(!lane.wait_text("Pressed for real", 300));
    assert_main_world_untouched(&mut lane);

    lane.capability
        .port_mut()
        .stop_inspect()
        .expect("stop inspect mode");
    mouse(&mut lane, "mouseMoved", x, y);
    assert_eq!(
        capture(&mut lane),
        plain,
        "leaving inspect mode clears the overlay"
    );
    mouse(&mut lane, "mouseMoved", x + 2.0, y);
    assert!(
        overlay_event(&mut lane, "Overlay.nodeHighlightRequested").is_none(),
        "no hover highlight after leaving"
    );
    assert_main_world_untouched(&mut lane);

    // The page takes clicks again.
    mouse(&mut lane, "mousePressed", x, y);
    mouse(&mut lane, "mouseReleased", x, y);
    assert!(lane.wait_text("Pressed for real", 2_000));
    assert!(
        lane.capability
            .port_mut()
            .transport
            .take_overlay_events()
            .is_empty()
    );
}

/// Annotate and the Agent's act highlight share Chromium's one overlay. An
/// Agent highlight pauses hover highlighting until Sage clears it (a navigation
/// starts or the Agent detaches), and leaving Annotate clears an Agent
/// highlight too.
#[test]
fn annotate_and_the_agent_highlight_share_one_overlay() {
    let mut lane = Lane::open("/hostile.html");
    let (_, _, x, y) = press_button(&mut lane);
    mouse(&mut lane, "mouseMoved", x, y);
    let plain = capture(&mut lane);
    lane.capability.port_mut().start_inspect().unwrap();
    let (_, backend, x, y) = press_button(&mut lane);
    lane.capability.port_mut().transport.take_overlay_events();
    // The parameters `cdp_port/highlight.rs` sends for an Agent act.
    let agent = json!({
        "backendNodeId": backend,
        "highlightConfig": {
            "showInfo": false,
            "contentColor": {"r": 74, "g": 143, "b": 255, "a": 0.24},
            "borderColor": {"r": 74, "g": 143, "b": 255, "a": 0.9}
        }
    });
    cdp(&mut lane, "Overlay.highlightNode", agent.clone());
    mouse(&mut lane, "mouseMoved", x + 3.0, y);
    assert!(
        overlay_event(&mut lane, "Overlay.nodeHighlightRequested").is_none(),
        "an Agent highlight pauses hover highlighting"
    );
    cdp(&mut lane, "Overlay.hideHighlight", json!({}));
    mouse(&mut lane, "mouseMoved", x + 1.0, y);
    assert!(
        overlay_event(&mut lane, "Overlay.nodeHighlightRequested").is_some(),
        "clearing the Agent highlight resumes hover highlighting"
    );

    cdp(&mut lane, "Overlay.highlightNode", agent);
    lane.capability.port_mut().stop_inspect().unwrap();
    mouse(&mut lane, "mouseMoved", x, y);
    assert_eq!(
        capture(&mut lane),
        plain,
        "leaving Annotate clears the Agent highlight"
    );
}
