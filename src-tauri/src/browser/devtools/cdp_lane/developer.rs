//! Developer mode (ADR-0009b) through the real dispatcher: a raw CDP command
//! reaches the page and its result reaches the Agent redacted and capped.
//! The gate's grant and deny list are covered by the permission tests; here
//! the command is already admitted.

use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};

use super::{Lane, handle};
use crate::acp::browser::developer::DEVELOPER_OBJECT_GROUP;
use crate::acp::browser::{BrowserAction, BrowserCommand, BrowserFailure, BrowserResult};
use crate::browser::devtools::transport::CdpTransport;

fn cdp(lane: &mut Lane, method: &str, params: Value) -> Result<BrowserResult, BrowserFailure> {
    lane.run(BrowserCommand::Cdp {
        method: method.into(),
        params: params.as_object().cloned().unwrap_or_else(Map::new),
    })
}

fn evaluate(lane: &mut Lane, expression: &str) -> Result<Value, BrowserFailure> {
    match cdp(
        lane,
        "Runtime.evaluate",
        json!({"expression": expression, "returnByValue": true}),
    )? {
        BrowserResult::Cdp { result } => Ok(result["result"]["value"].clone()),
        other => panic!("cdp result: {other:?}"),
    }
}

#[test]
fn a_script_runs_in_the_page_and_its_value_comes_back() {
    let mut lane = Lane::open("/form.html");
    assert_eq!(
        evaluate(&mut lane, "document.title.length > 0 && 6 * 7").unwrap(),
        json!(42)
    );
    evaluate(&mut lane, "window.__developerMark = 'set'").unwrap();
    assert_eq!(lane.evaluate("window.__developerMark"), json!("set"));
}

#[test]
fn a_raw_command_ends_the_handles_a_snapshot_issued() {
    let mut lane = Lane::open("/form.html");
    let (_, _, nodes) = lane.snapshot();
    let button = handle(&nodes, "button", "Submit");
    evaluate(&mut lane, "document.body.dataset.touched = '1'").unwrap();
    assert_eq!(
        lane.act(BrowserAction::Click { handle: button }),
        Err(BrowserFailure::StaleHandle)
    );
}

#[test]
fn secrets_in_a_raw_result_are_redacted_before_the_agent_sees_them() {
    let mut lane = Lane::open("/form.html");
    let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U";
    let value = evaluate(
        &mut lane,
        &format!("({{ password: 'hunter2-canary', note: 'Bearer {jwt}' }})"),
    )
    .unwrap();
    assert_eq!(value["password"], "[redacted]");
    lane.assert_never_leaked(&["hunter2-canary", jwt]);
}

#[test]
fn a_result_over_the_cap_fails_and_a_bad_method_is_refused() {
    let mut lane = Lane::open("/form.html");
    assert_eq!(
        evaluate(&mut lane, "'x'.repeat(300 * 1024)"),
        Err(BrowserFailure::Capacity)
    );
    assert_eq!(
        cdp(&mut lane, "Runtime.noSuchCommand", json!({})),
        Err(BrowserFailure::Unavailable)
    );
}

/// Starts an evaluation that outlives its caller's timeout and leaves a
/// remote object in `group`, reachable from the page only through a WeakRef.
fn evaluate_past_its_timeout(lane: &mut Lane, group: &str, marker: &str) {
    let expression = format!(
        "(() => {{ const held = {{ blob: new ArrayBuffer(1 << 20) }}; \
         window.{marker} = new WeakRef(held); \
         const end = Date.now() + 1500; while (Date.now() < end) {{}} return held; }})()"
    );
    let params = json!({"expression": expression, "objectGroup": group});
    assert!(
        lane.capability
            .port_mut()
            .transport
            .call(
                "Runtime.evaluate",
                &params.to_string(),
                65_536,
                Duration::from_millis(200),
            )
            .is_err(),
        "the caller stops waiting before the page answers"
    );
}

fn collected(lane: &mut Lane, marker: &str) -> bool {
    lane.capability
        .port_mut()
        .transport
        .observe(
            "HeapProfiler.collectGarbage",
            "{}",
            4096,
            Duration::from_secs(5),
        )
        .expect("collect garbage");
    lane.evaluate(&format!("window.{marker}.deref() === undefined")) == json!(true)
}

#[test]
fn a_late_evaluation_answer_still_releases_its_objects() {
    let mut lane = Lane::open("/form.html");
    // Control: an evaluation outside developer mode's group keeps its object.
    evaluate_past_its_timeout(&mut lane, "kept-by-test", "__kept");
    evaluate_past_its_timeout(&mut lane, DEVELOPER_OBJECT_GROUP, "__released");
    let deadline = Instant::now() + Duration::from_secs(10);
    while lane.capability.port_mut().transport.completion_releases() == 0 {
        assert!(Instant::now() < deadline, "the late answer was released");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        lane.capability.port_mut().transport.completion_releases(),
        1
    );
    assert!(!collected(&mut lane, "__kept"));
    assert!(collected(&mut lane, "__released"));
}
