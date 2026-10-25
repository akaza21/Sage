//! Security item S8: the Agent's scripts run in an isolated world, so a page
//! that overrides its own globals can neither lie to them nor see them run.

use serde_json::json;

use super::{Lane, handle, page_context};
use crate::acp::browser::{BrowserAction, BrowserResult, FillSecret};

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
fn s8_click_and_fill_use_pristine_dom_methods_and_hide_the_value_from_page_hooks() {
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
        lane.evaluate("document.getElementById('name').value"),
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
