//! Developer mode (ADR-0009b) through the real dispatcher: a raw CDP command
//! reaches the page and its result reaches the Agent redacted and capped.
//! The gate's grant and deny list are covered by the permission tests; here
//! the command is already admitted.

use serde_json::{Map, Value, json};

use super::{Lane, handle};
use crate::acp::browser::{BrowserAction, BrowserCommand, BrowserFailure, BrowserResult};

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
