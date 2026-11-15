use serde_json::{Value, json};

use super::tests::{initialized, prompt};
use super::*;
use crate::acp::bindings::claude::runtime::{ChildEvent, ChildOutcome, OPEN_CHILDREN};

fn spawn(bridge: &mut SdkBridge, id: &str) -> Vec<ChildEvent> {
    bridge
        .inbound(json!({"type":"assistant","message":{"id":format!("m-{id}"),"content":[
            {"type":"tool_use","id":id,"name":"Agent","input":{"description":"Look around","prompt":"Look"}}]}}))
        .unwrap()
        .children
}

fn tool_result(bridge: &mut SdkBridge, id: &str, output: Value) -> Vec<ChildEvent> {
    bridge
        .inbound(json!({"type":"user","message":{"content":[
            {"type":"tool_result","tool_use_id":id,"content":"Async agent launched successfully."}]},
            "tool_use_result":output}))
        .unwrap()
        .children
}

fn task(bridge: &mut SdkBridge, frame: Value) -> Vec<ChildEvent> {
    bridge.inbound(frame).unwrap().children
}

fn launched(agent: &str) -> Value {
    json!({"status":"async_launched","agentId":agent,"description":"Look around","prompt":"Look"})
}

fn notification(task_id: &str, tool_use_id: Option<&str>, status: &str) -> Value {
    let mut frame = json!({"type":"system","subtype":"task_notification","task_id":task_id,
        "status":status,"output_file":"","summary":"Done"});
    if let Some(id) = tool_use_id {
        frame["tool_use_id"] = id.into();
    }
    frame
}

fn ended(events: &[ChildEvent]) -> Vec<(&str, ChildOutcome)> {
    events
        .iter()
        .filter_map(|event| match event {
            ChildEvent::Ended { key, outcome } => Some((key.as_str(), *outcome)),
            _ => None,
        })
        .collect()
}

fn finish(bridge: &mut SdkBridge) -> Vec<ChildEvent> {
    task(
        bridge,
        json!({"type":"result","subtype":"success","is_error":false,"usage":{}}),
    )
}

#[test]
fn ctrl_b_backgrounding_keeps_a_running_subagent_open_past_its_result_and_turn() {
    let mut bridge = initialized();
    prompt(&mut bridge, 3);
    spawn(&mut bridge, "toolu_a");
    task(
        &mut bridge,
        json!({"type":"system","subtype":"task_started","task_id":"t1","tool_use_id":"toolu_a",
            "description":"Look around","is_backgrounded":false}),
    );
    task(
        &mut bridge,
        json!({"type":"system","subtype":"task_updated","task_id":"t1","patch":{"is_backgrounded":true}}),
    );
    let result = tool_result(&mut bridge, "toolu_a", json!({"status":"backgrounded"}));
    assert_eq!(ended(&result), []);
    assert_eq!(ended(&finish(&mut bridge)), []);
    let settled = task(&mut bridge, notification("t1", None, "failed"));
    assert_eq!(ended(&settled), [("toolu_a", ChildOutcome::Failed)]);
}

#[test]
fn a_notification_naming_only_the_task_settles_a_subagent_launched_in_the_background() {
    let mut bridge = initialized();
    prompt(&mut bridge, 3);
    spawn(&mut bridge, "toolu_a");
    spawn(&mut bridge, "toolu_b");
    tool_result(&mut bridge, "toolu_a", launched("agent-a"));
    tool_result(&mut bridge, "toolu_b", launched("agent-b"));
    finish(&mut bridge);
    // No task_started ever arrived, so only the launch names the task.
    let settled = task(&mut bridge, notification("agent-b", None, "completed"));
    assert_eq!(ended(&settled), [("toolu_b", ChildOutcome::Completed)]);
    // A subagent's own frames name its task too.
    bridge
        .inbound(
            json!({"type":"assistant","parent_tool_use_id":"toolu_a","agent_id":"agent-a",
            "uuid":"u1","message":{"id":"s1","content":[{"type":"text","text":"Still looking"}]}}),
        )
        .unwrap();
    let settled = task(&mut bridge, notification("agent-a", None, "stopped"));
    assert_eq!(ended(&settled), [("toolu_a", ChildOutcome::Stopped)]);
    assert_eq!(
        ended(&task(
            &mut bridge,
            notification("unknown", None, "completed")
        )),
        []
    );
}

#[test]
fn a_foreground_subagent_ends_on_its_result_and_ignores_a_late_notification() {
    let mut bridge = initialized();
    prompt(&mut bridge, 3);
    spawn(&mut bridge, "toolu_a");
    task(
        &mut bridge,
        json!({"type":"system","subtype":"task_started","task_id":"t1","tool_use_id":"toolu_a",
            "description":"Look around","is_backgrounded":false}),
    );
    let result = tool_result(&mut bridge, "toolu_a", json!({"status":"completed"}));
    assert_eq!(ended(&result), [("toolu_a", ChildOutcome::Completed)]);
    let late = task(&mut bridge, notification("t1", Some("toolu_a"), "failed"));
    assert!(late.is_empty(), "{late:?}");
}

#[test]
fn interrupting_the_parent_turn_stops_foreground_subagents_but_not_background_ones() {
    let mut bridge = initialized();
    prompt(&mut bridge, 3);
    spawn(&mut bridge, "toolu_bg");
    tool_result(&mut bridge, "toolu_bg", launched("agent-bg"));
    spawn(&mut bridge, "toolu_fg");
    let interrupt = bridge
        .outbound(
            &json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":super::tests::SESSION}}),
        )
        .unwrap();
    bridge
        .inbound(
            json!({"type":"control_response","response":{"subtype":"success",
            "request_id":interrupt["request_id"],"response":{}}}),
        )
        .unwrap();
    let cancelled = bridge
        .inbound(
            json!({"type":"result","subtype":"error_during_execution","is_error":true,"usage":{}}),
        )
        .unwrap();
    assert_eq!(cancelled.acp[0]["result"]["stopReason"], "cancelled");
    assert_eq!(
        ended(&cancelled.children),
        [("toolu_fg", ChildOutcome::Stopped)]
    );
    let settled = task(&mut bridge, notification("agent-bg", None, "completed"));
    assert_eq!(ended(&settled), [("toolu_bg", ChildOutcome::Completed)]);
}

#[test]
fn held_background_subagents_count_against_the_open_transcript_cap_until_they_settle() {
    let mut bridge = initialized();
    prompt(&mut bridge, 3);
    for index in 0..OPEN_CHILDREN {
        let id = format!("toolu_{index}");
        spawn(&mut bridge, &id);
        tool_result(&mut bridge, &id, launched(&format!("agent-{index}")));
    }
    finish(&mut bridge);
    prompt(&mut bridge, 4);
    let over = spawn(&mut bridge, "toolu_over");
    assert!(
        over.is_empty(),
        "past the cap it stays a tool row: {over:?}"
    );
    task(&mut bridge, notification("agent-0", None, "completed"));
    let room = spawn(&mut bridge, "toolu_next");
    assert!(
        matches!(&room[..], [ChildEvent::Spawned { key, .. }] if key == "toolu_next"),
        "{room:?}"
    );
}
