//! Security item S8: the Agent's scripts run in an isolated world, so a page
//! that overrides its own globals can neither lie to them nor see them run.

use std::time::Duration;

use serde_json::{Value, json};

use super::{CdpTransport, Lane, handle, page_context};
use crate::acp::browser::{BrowserAction, BrowserFailure, BrowserResult, FillSecret};

const FAKE: &str = "FAKE-PHRASE-9d1";
const TRUTH: &str = "hostile-truth-41";

/// No page hook saw the Agent, and the Agent left no global on the page.
fn assert_unobserved(lane: &mut Lane) {
    assert_eq!(lane.evaluate("window.__agentSeen"), json!([]));
    assert_eq!(
        lane.evaluate("Object.getOwnPropertyNames(window).length - window.__globalsAtLoad"),
        json!(0)
    );
}

fn isolated_worlds(lane: &mut Lane) -> usize {
    lane.capability
        .port_mut()
        .transport
        .calls("Page.createIsolatedWorld")
}

#[test]
fn s8_text_wait_reads_the_real_page_under_overridden_globals() {
    let mut lane = Lane::open("/hostile.html");
    assert!(!lane.wait_text(FAKE, 300));
    assert!(lane.wait_text(TRUTH, 2_000));
    assert_unobserved(&mut lane);
}

#[test]
fn s8_page_context_reports_the_real_title_and_selection() {
    let mut lane = Lane::open("/hostile.html");
    let revision = lane
        .capability
        .port_mut()
        .stamp()
        .unwrap()
        .generation
        .to_string();
    let url = lane.site.url("/hostile.html");
    let context = page_context::capture_page_context(
        lane.capability.port_mut(),
        "browser-1",
        &revision,
        || Ok(url),
    )
    .expect("page context");
    let context = serde_json::to_value(&context).unwrap();
    assert_eq!(context["title"], "Hostile page");
    let text = context["text"].as_str().unwrap();
    assert!(
        text.starts_with(&format!("Selected text:\n{TRUTH}\n")),
        "{text}"
    );
    assert!(!context.to_string().contains("FAKE-"), "{context}");
    assert_unobserved(&mut lane);
}

#[test]
fn s8_click_and_fill_use_pristine_dom_methods_and_hide_the_value_from_page_and_instance_hooks() {
    let secret = "S8-FILL-6be0";
    let mut lane = Lane::open("/hostile.html");
    let (_, _, nodes) = lane.snapshot();
    let click = lane.act(BrowserAction::Click {
        handle: handle(&nodes, "button", "Press me"),
    });
    assert_eq!(click, Ok(BrowserResult::Act { applied: true }));
    assert!(lane.wait_text("Pressed for real", 2_000));
    let fill = lane.act(BrowserAction::Fill {
        handle: handle(&nodes, "textbox", "Name"),
        value: FillSecret::new(secret.into()),
    });
    assert_eq!(fill, Ok(BrowserResult::Act { applied: true }));
    assert!(lane.wait_text("name holds 12 characters", 2_000));
    assert_eq!(
        lane.evaluate("window.__valueOf(document.getElementById('name'))"),
        secret
    );
    assert_unobserved(&mut lane);
    lane.assert_never_leaked(&[secret]);
}

#[test]
fn s8_isolated_world_is_made_once_per_document_and_again_after_each_navigation() {
    let mut lane = Lane::open("/hostile.html");
    assert!(lane.wait_text(TRUTH, 2_000));
    let (_, _, nodes) = lane.snapshot();
    let click = lane.act(BrowserAction::Click {
        handle: handle(&nodes, "button", "Press me"),
    });
    assert_eq!(click, Ok(BrowserResult::Act { applied: true }));
    assert!(lane.wait_text("Pressed for real", 2_000));
    assert_eq!(isolated_worlds(&mut lane), 1);

    lane.navigate("/navigates.html");
    assert!(lane.wait_text("Departure", 2_000));
    assert_eq!(isolated_worlds(&mut lane), 2);

    // A navigation the page starts itself needs a world in the new document.
    let (_, _, nodes) = lane.snapshot();
    let go = lane.act(BrowserAction::Click {
        handle: handle(&nodes, "link", "Go to landing"),
    });
    assert_eq!(go, Ok(BrowserResult::Act { applied: true }));
    assert!(lane.wait_text("Landed safely", 5_000));
    assert_eq!(isolated_worlds(&mut lane), 3);

    lane.navigate("/hostile.html");
    assert!(!lane.wait_text(FAKE, 300));
    assert!(lane.wait_text(TRUTH, 2_000));
    assert_eq!(isolated_worlds(&mut lane), 4);
    assert_unobserved(&mut lane);
}

#[test]
fn s8_scroll_uses_the_pristine_scroll_by() {
    let mut lane = Lane::open("/hostile.html");
    let (_, _, nodes) = lane.snapshot();
    let scroll = lane.act(BrowserAction::Scroll {
        handle: handle(&nodes, "textbox", "Log"),
        delta_x: 0,
        delta_y: 60,
    });
    assert_eq!(scroll, Ok(BrowserResult::Act { applied: true }));
    let top = lane.evaluate("document.getElementById('log').scrollTop");
    assert!(top.as_f64().unwrap() > 0.0, "{top}");
    assert_unobserved(&mut lane);
}

#[test]
fn s8_a_page_with_a_huge_url_and_frame_tree_cannot_starve_agent_scripts() {
    let mut lane = Lane::open("/hostile.html");
    lane.evaluate("window.__growFrameTree()");
    let tree = lane
        .capability
        .port_mut()
        .transport
        .observe(
            "Page.getFrameTree",
            "{}",
            16 * 1_048_576,
            Duration::from_secs(5),
        )
        .expect("frame tree");
    let frames = serde_json::from_str::<Value>(&tree).unwrap()["frameTree"]["childFrames"]
        .as_array()
        .map_or(0, Vec::len);
    assert!(
        tree.len() > 65_536 && frames == 8,
        "{} bytes, {frames} frames",
        tree.len()
    );
    assert!(lane.wait_text(TRUTH, 2_000));
    assert!(!lane.wait_text(FAKE, 1_000));
    let (_, _, nodes) = lane.snapshot();
    let click = lane.act(BrowserAction::Click {
        handle: handle(&nodes, "button", "Press me"),
    });
    assert_eq!(click, Ok(BrowserResult::Act { applied: true }));
    assert!(lane.wait_text("Pressed for real", 2_000));
    assert_unobserved(&mut lane);
}

#[test]
fn s8_a_world_whose_document_went_away_is_made_again() {
    let mut lane = Lane::open("/hostile.html");
    assert!(lane.wait_text(TRUTH, 2_000));
    assert_eq!(isolated_worlds(&mut lane), 1);
    // The page replaces its document before the reported stamp moves, as
    // WebView2 reports a new document only after its navigation events.
    let held = lane.capability.port_mut().stamp().unwrap();
    lane.capability
        .port_mut()
        .transport
        .hold_stamp(Some(Ok(held)));
    lane.evaluate("location.href = '/landing.html'");
    assert!(lane.wait_text("Landed safely", 5_000));
    assert_eq!(isolated_worlds(&mut lane), 2);
    lane.capability.port_mut().transport.hold_stamp(None);
}

/// Starts a page-initiated navigation to `path` with the stamp unavailable
/// until the test releases it, as WebView2 reports a loading tab.
fn navigate_while_loading(lane: &mut Lane, path: &str) {
    lane.capability
        .port_mut()
        .transport
        .hold_stamp(Some(Err(BrowserFailure::Unavailable)));
    lane.evaluate(&format!("location.href = '{path}'"));
}

/// The old page and the slow new page both show "Shared phrase"; the new
/// page keeps loading on a hung image after it commits.
fn assert_wait_skips_the_outgoing_page(lane: &mut Lane) {
    assert!(!lane.wait_text("Shared phrase", 1_000));
    // Probes held by the pending navigation went without the dispatch permit.
    assert!(!lane.capability.port_mut().transport.issued());
    assert!(lane.wait_text("New page", 10_000));
    assert!(!lane.capability.port_mut().transport.issued());
    assert!(lane.wait_text("Shared phrase", 1_000));
    lane.capability.port_mut().transport.hold_stamp(None);
}

#[test]
fn s8_a_wait_while_loading_skips_an_outgoing_page_the_agent_scripted() {
    let mut lane = Lane::open("/shared-old.html");
    assert!(lane.wait_text("Old page", 2_000));
    navigate_while_loading(&mut lane, "/shared-new.html");
    assert_wait_skips_the_outgoing_page(&mut lane);
}

#[test]
fn s8_a_wait_while_loading_skips_an_outgoing_page_the_agent_never_scripted() {
    let mut lane = Lane::open("/shared-old.html");
    navigate_while_loading(&mut lane, "/shared-new.html");
    assert_wait_skips_the_outgoing_page(&mut lane);
}
