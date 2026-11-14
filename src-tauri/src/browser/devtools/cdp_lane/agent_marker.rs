use serde_json::json;

use super::{
    BrowserAction, BrowserResult, Lane,
    annotate::{assert_main_world_untouched, cdp, watch_main_world},
    handle,
};
use crate::browser::devtools::cdp_port::highlight::{self, CLEAR};

fn capture(lane: &mut Lane) -> String {
    cdp(lane, "Page.captureScreenshot", json!({"format": "png"}))["data"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn button(lane: &mut Lane) -> u64 {
    let root = cdp(lane, "DOM.getDocument", json!({"depth": 0}))["root"]["nodeId"].clone();
    let node = cdp(
        lane,
        "DOM.querySelector",
        json!({"nodeId": root, "selector": "button"}),
    )["nodeId"]
        .clone();
    cdp(lane, "DOM.describeNode", json!({"nodeId": node}))["node"]["backendNodeId"]
        .as_u64()
        .unwrap()
}

#[test]
fn the_marker_outlines_the_target_with_a_cursor_and_clearing_restores_the_page() {
    let mut lane = Lane::open("/hostile.html");
    watch_main_world(&mut lane);
    let plain = capture(&mut lane);
    let backend = button(&mut lane);
    let port = lane.capability.port_mut();
    let stamp = port.stamp().unwrap();
    assert!(highlight::mark(port, stamp, backend));
    let marked = capture(&mut lane);
    assert_ne!(marked, plain, "the marker outlines its target");

    let [(hide_highlight, _), (hide_cursor, _)] = CLEAR;
    cdp(&mut lane, hide_highlight, json!({}));
    let cursor_only = capture(&mut lane);
    assert_ne!(cursor_only, plain, "the cursor dot is drawn on its own");
    assert_ne!(cursor_only, marked);
    cdp(&mut lane, hide_cursor, json!({}));
    assert_eq!(capture(&mut lane), plain, "clearing restores the page");

    let port = lane.capability.port_mut();
    assert!(highlight::mark(port, stamp, backend));
    assert!(matches!(
        lane.screenshot(None),
        Ok(BrowserResult::Screenshot { .. })
    ));
    assert_eq!(capture(&mut lane), plain, "a capture leaves no marker");
    assert_main_world_untouched(&mut lane);
}

#[test]
fn an_agent_act_leaves_no_marker_and_turns_the_overlay_off_again() {
    let mut lane = Lane::open("/hostile.html");
    watch_main_world(&mut lane);
    let plain = capture(&mut lane);
    let (_, _, nodes) = lane.snapshot();
    let scroll = lane.act(BrowserAction::Scroll {
        handle: handle(&nodes, "button", "Press me"),
        delta_x: 0,
        delta_y: 0,
    });
    assert_eq!(scroll, Ok(BrowserResult::Act { applied: true }));
    let transport = &lane.capability.port_mut().transport;
    assert_eq!(
        transport.calls("Overlay.highlightNode"),
        1,
        "the act was marked"
    );
    assert_eq!(transport.calls("Overlay.disable"), 1);
    assert_eq!(transport.calls("DOM.disable"), 1);
    assert_eq!(capture(&mut lane), plain, "the act's marker is gone");
    assert_main_world_untouched(&mut lane);
}
