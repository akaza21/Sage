//! The Agent act marker (H3): an act outlines its target and puts the cursor
//! dot on the target's centre, both in Chromium's overlay. Clearing it, as
//! Stop, Take over, detach and navigation do, restores the exact page pixels,
//! and the page's main world sees none of it.

use serde_json::json;

use super::{
    BrowserAction, BrowserResult, Lane,
    annotate::{assert_main_world_untouched, cdp, watch_main_world},
    handle,
};
use crate::browser::devtools::cdp_port::highlight::CLEAR;

fn capture(lane: &mut Lane) -> String {
    cdp(lane, "Page.captureScreenshot", json!({"format": "png"}))["data"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn an_agent_act_marks_its_target_with_a_cursor_and_clearing_restores_the_page() {
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
    let marked = capture(&mut lane);
    assert_ne!(marked, plain, "the act outlines its target");

    let [(hide_highlight, _), (hide_cursor, _)] = CLEAR;
    assert_eq!(hide_highlight, "Overlay.hideHighlight");
    cdp(&mut lane, hide_highlight, json!({}));
    let cursor_only = capture(&mut lane);
    assert_ne!(cursor_only, plain, "the cursor dot is drawn on its own");
    assert_ne!(cursor_only, marked);
    cdp(&mut lane, hide_cursor, json!({}));
    assert_eq!(capture(&mut lane), plain, "clearing restores the page");
    assert_main_world_untouched(&mut lane);
}
