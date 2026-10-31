//! The CDP lane: the Agent browser command layer driven through the real
//! `DevtoolsCapability` dispatcher against headless Chromium and a loopback
//! fixture site. Built only with `--features cdp-lane` on unix; a missing
//! Chromium fails these tests rather than skipping them.
//!
//! What the lane does not prove: it calls `contract::dispatch` directly, so the
//! WebView2 dispatch permit, revocation during navigation, the outer dispatch
//! deadline and cancellation stay covered only by the Windows build. The lane
//! validates CDP parameters, result parsing and what reaches the Agent.

mod agent_marker;
mod annotate;
mod chromium;
mod isolation;
mod select;
mod site;

use std::time::Duration;

use serde_json::Value;

use super::{
    capability::DevtoolsCapability, cdp_port::CdpPort, cdp_port::page_context,
    screenshot::admit_jpeg, transport::CdpTransport,
};
use crate::acp::{
    browser::{
        BrowserAction, BrowserCommand, BrowserFailure, BrowserRequest, BrowserResult, BrowserScope,
        ElementHandle, FillSecret, SnapshotNode, ViewportRegion, WaitCondition, dispatch,
    },
    limits::BROWSER_LIMITS,
};
use chromium::Chromium;
use site::Site;

const PASSWORD_CANARY: &str = "CANARY-PW-5f1e9c";
const CSRF_CANARY: &str = "CANARY-CSRF-a83d27";

struct Lane {
    site: Site,
    capability: DevtoolsCapability<CdpPort<Chromium>>,
    scope: BrowserScope,
    /// Every dispatcher response this test produced, as the Agent sees it.
    transcript: Vec<String>,
}

impl Lane {
    fn open(path: &str) -> Self {
        let scope = BrowserScope {
            session_id: "cdp-lane".into(),
            tab_id: 1,
        };
        let mut lane = Self {
            site: Site::start(),
            capability: DevtoolsCapability::new(scope.clone(), CdpPort::new(Chromium::launch())),
            scope,
            transcript: Vec::new(),
        };
        lane.navigate(path);
        lane
    }

    fn run(&mut self, command: BrowserCommand) -> Result<BrowserResult, BrowserFailure> {
        let request = BrowserRequest {
            scope: self.scope.clone(),
            command,
        };
        self.capability.port_mut().transport.begin_dispatch();
        let response = dispatch(&mut self.capability, request);
        self.transcript.push(match &response {
            Ok(response) => serde_json::to_string(response).expect("response JSON"),
            Err(failure) => format!("{failure:?}"),
        });
        response.map(|response| response.result)
    }

    fn navigate(&mut self, path: &str) -> u64 {
        let url = self.site.url(path);
        match self.run(BrowserCommand::Navigate { url: url.clone() }) {
            Ok(BrowserResult::Navigate {
                url: settled,
                navigation_epoch,
            }) => {
                assert_eq!(settled, url);
                navigation_epoch
            }
            other => panic!("navigate to {path}: {other:?}"),
        }
    }

    fn snapshot(&mut self) -> (u64, String, Vec<SnapshotNode>) {
        match self.run(BrowserCommand::SnapshotPage {}) {
            Ok(BrowserResult::SnapshotPage {
                navigation_epoch,
                text,
                nodes,
            }) => (navigation_epoch, text, nodes),
            other => panic!("snapshot: {other:?}"),
        }
    }

    fn act(&mut self, action: BrowserAction) -> Result<BrowserResult, BrowserFailure> {
        self.run(BrowserCommand::Act { action })
    }

    fn wait_text(&mut self, text: &str, timeout_ms: u32) -> bool {
        match self.run(BrowserCommand::WaitFor {
            condition: WaitCondition::Text { text: text.into() },
            timeout_ms,
        }) {
            Ok(BrowserResult::WaitFor { matched }) => matched,
            other => panic!("wait for {text:?}: {other:?}"),
        }
    }

    fn screenshot(
        &mut self,
        region: Option<ViewportRegion>,
    ) -> Result<BrowserResult, BrowserFailure> {
        self.run(BrowserCommand::Screenshot { region })
    }

    /// A test-side observation of the page, outside the Agent transcript.
    fn evaluate(&mut self, expression: &str) -> Value {
        let params = serde_json::json!({"expression": expression, "returnByValue": true});
        let result = self
            .capability
            .port_mut()
            .transport
            .observe(
                "Runtime.evaluate",
                &params.to_string(),
                65_536,
                Duration::from_secs(5),
            )
            .expect("evaluate");
        serde_json::from_str::<Value>(&result).expect("evaluate result")["result"]["value"].clone()
    }

    fn assert_never_leaked(&self, secrets: &[&str]) {
        for entry in &self.transcript {
            for secret in secrets {
                assert!(!entry.contains(secret), "{secret} leaked into {entry}");
            }
        }
    }
}

fn handle(nodes: &[SnapshotNode], role: &str, name: &str) -> ElementHandle {
    nodes
        .iter()
        .find(|node| node.role == role && node.name.contains(name))
        .unwrap_or_else(|| panic!("no {role} named {name:?} in {nodes:?}"))
        .handle
        .clone()
}

#[test]
fn snapshot_of_a_ten_thousand_node_page_fails_closed_within_its_caps() {
    let mut lane = Lane::open("/big.html");
    let dom_nodes = lane.evaluate(
        "(() => { let count = 0; const walker = document.createTreeWalker(document, NodeFilter.SHOW_ALL); while (walker.nextNode()) count++; return count; })()",
    );
    assert!(dom_nodes.as_u64().unwrap() >= 10_000, "{dom_nodes}");
    let raw = lane
        .capability
        .port_mut()
        .transport
        .observe(
            "Accessibility.getFullAXTree",
            r#"{"depth":16}"#,
            16 * 1_048_576,
            Duration::from_secs(10),
        )
        .expect("raw accessibility tree");
    let raw_nodes = serde_json::from_str::<Value>(&raw).unwrap()["nodes"]
        .as_array()
        .unwrap()
        .len();
    // Known limitation: the whole tree is fetched before projection, so a
    // page this large exceeds the 1 MiB fetch cap or the node admission cap
    // and the snapshot fails closed instead of returning the first nodes.
    assert!(
        raw.len() > 1_048_576 || raw_nodes > BROWSER_LIMITS.snapshot_nodes * 16,
        "{} bytes, {raw_nodes} nodes",
        raw.len()
    );
    assert_eq!(
        lane.run(BrowserCommand::SnapshotPage {}),
        Err(BrowserFailure::Capacity)
    );
    // The failure leaves the page usable within the same caps.
    lane.navigate("/landing.html");
    let (_, text, nodes) = lane.snapshot();
    assert!(nodes.len() <= BROWSER_LIMITS.snapshot_nodes);
    assert!(text.len() <= BROWSER_LIMITS.snapshot_text_bytes);
    assert!(text.contains("Landed safely"));
}

#[test]
fn click_fill_and_key_reach_the_page_and_fill_never_echoes_its_value() {
    let secret = "FILL-SECRET-0c4b7e";
    let mut lane = Lane::open("/form.html");
    let (_, _, nodes) = lane.snapshot();
    let click = lane.act(BrowserAction::Click {
        handle: handle(&nodes, "button", "Submit"),
    });
    assert_eq!(click, Ok(BrowserResult::Act { applied: true }));
    assert!(lane.wait_text("Submitted once", 2_000));
    let fill = lane.act(BrowserAction::Fill {
        handle: handle(&nodes, "textbox", "Email"),
        value: FillSecret::new(secret.into()),
    });
    assert_eq!(fill, Ok(BrowserResult::Act { applied: true }));
    assert!(lane.wait_text("email now holds 18 characters", 2_000));
    assert_eq!(
        lane.evaluate("document.getElementById('email').value"),
        secret
    );
    let key = lane.act(BrowserAction::Key {
        handle: handle(&nodes, "textbox", "Notes"),
        key: "Enter".into(),
    });
    assert_eq!(key, Ok(BrowserResult::Act { applied: true }));
    assert!(lane.wait_text("Notes saw Enter", 2_000));
    lane.assert_never_leaked(&[secret]);

    // Known limitation: a plain text field renders its content, so a later
    // snapshot reads the filled value back as page text. Only password
    // fields are masked (see the canary test).
    let (_, text, _) = lane.snapshot();
    assert!(text.contains(secret));
}

#[test]
fn password_and_hidden_token_canaries_never_leave_the_page() {
    let typed = "CANARY-TYPED-PW-e40d";
    let mut lane = Lane::open("/form.html");
    let (_, _, nodes) = lane.snapshot();
    let password = handle(&nodes, "textbox", "Password");
    let fill = lane.act(BrowserAction::Fill {
        handle: password,
        value: FillSecret::new(typed.into()),
    });
    assert_eq!(fill, Ok(BrowserResult::Act { applied: true }));
    assert!(lane.wait_text("password now holds 20 characters", 2_000));
    lane.snapshot();
    let revision = lane
        .capability
        .port_mut()
        .stamp()
        .unwrap()
        .generation
        .to_string();
    let url = lane.site.url("/form.html");
    let context = page_context::capture_page_context(
        lane.capability.port_mut(),
        "browser-1",
        &revision,
        || Ok(url),
    )
    .expect("page context");
    let context = serde_json::to_string(&context).unwrap();
    assert!(context.contains("Sign in"));
    lane.transcript.push(context);
    lane.assert_never_leaked(&[PASSWORD_CANARY, CSRF_CANARY, typed]);
}

#[test]
fn wait_for_text_matches_visible_text_and_times_out_otherwise() {
    let mut lane = Lane::open("/landing.html");
    assert!(lane.wait_text("Landed safely", 2_000));
    assert!(!lane.wait_text("A phrase this page never shows", 300));
}

#[test]
fn screenshot_admits_the_viewport_and_refuses_captures_beyond_browser_limits() {
    let mut lane = Lane::open("/landing.html");
    let Ok(BrowserResult::Screenshot {
        width,
        height,
        jpeg_base64,
    }) = lane.screenshot(None)
    else {
        panic!("viewport screenshot");
    };
    assert!(width <= BROWSER_LIMITS.screenshot_width);
    assert!(height <= BROWSER_LIMITS.screenshot_height);
    assert!(jpeg_base64.len() <= BROWSER_LIMITS.screenshot_bytes);
    assert_eq!(admit_jpeg(width, height, &jpeg_base64), Ok(()));
    let region = ViewportRegion {
        x: 10,
        y: 10,
        width: 200,
        height: 100,
    };
    assert!(matches!(
        lane.screenshot(Some(region)),
        Ok(BrowserResult::Screenshot {
            width: 200,
            height: 100,
            ..
        })
    ));
    let beyond_viewport = ViewportRegion {
        x: width,
        y: 0,
        width: 10,
        height: 10,
    };
    assert_eq!(
        lane.screenshot(Some(beyond_viewport)),
        Err(BrowserFailure::InvalidRequest)
    );
    let oversized = ViewportRegion {
        x: 0,
        y: 0,
        width: BROWSER_LIMITS.screenshot_width + 1,
        height: 10,
    };
    assert_eq!(
        lane.screenshot(Some(oversized)),
        Err(BrowserFailure::Capacity)
    );
    lane.capability
        .port_mut()
        .transport
        .observe(
            "Emulation.setDeviceMetricsOverride",
            r#"{"width":2400,"height":1400,"deviceScaleFactor":1,"mobile":false}"#,
            4096,
            Duration::from_secs(5),
        )
        .expect("grow the viewport");
    assert_eq!(lane.screenshot(None), Err(BrowserFailure::Capacity));
}

#[test]
fn handles_from_before_a_navigation_fail_as_stale() {
    let mut lane = Lane::open("/navigates.html");
    let (_, _, nodes) = lane.snapshot();
    let stay = handle(&nodes, "button", "Stay here");
    lane.navigate("/landing.html");
    assert_eq!(
        lane.act(BrowserAction::Click { handle: stay }),
        Err(BrowserFailure::StaleHandle)
    );

    // A navigation the page starts itself leaves the handles in place, so
    // the document stamp has to catch it.
    lane.navigate("/navigates.html");
    let (before, _, nodes) = lane.snapshot();
    let stay = handle(&nodes, "button", "Stay here");
    let go = lane.act(BrowserAction::Click {
        handle: handle(&nodes, "link", "Go to landing"),
    });
    assert_eq!(go, Ok(BrowserResult::Act { applied: true }));
    assert!(lane.wait_text("Landed safely", 5_000));
    let (after, _, _) = lane.snapshot();
    assert!(after > before);
    assert_eq!(
        lane.act(BrowserAction::Click { handle: stay }),
        Err(BrowserFailure::StaleHandle)
    );
}

#[test]
fn prompt_injection_text_comes_back_as_bounded_page_data_only() {
    let mut lane = Lane::open("/injection.html");
    let before = lane.capability.port_mut().stamp().unwrap();
    let (_, text, nodes) = lane.snapshot();
    assert!(text.contains("SYSTEM OVERRIDE"));
    assert!(text.contains("secret=[redacted]"));
    for hidden in [
        "INJECT-SECRET-77e1",
        "HIDDEN-INJECTION",
        "ARIA-HIDDEN-INJECTION",
    ] {
        assert!(!text.contains(hidden), "{hidden} in {text}");
        assert!(nodes.iter().all(|node| !node.name.contains(hidden)));
    }
    assert_eq!(lane.capability.port_mut().stamp().unwrap(), before);
    lane.assert_never_leaked(&["INJECT-SECRET-77e1", "HIDDEN-INJECTION"]);
}

/// Security item S8: the text wait runs in an isolated world, so a page that
/// replaces `String.prototype.includes` cannot fake a match.
#[test]
fn s8_text_wait_ignores_a_page_that_replaces_includes() {
    let phrase = "phrase-the-page-never-shows";
    let mut lane = Lane::open("/includes.html");
    let (_, text, _) = lane.snapshot();
    assert!(!text.contains(phrase));
    assert!(!lane.wait_text(phrase, 300));
    assert!(lane.wait_text("This page never shows", 2_000));
}
