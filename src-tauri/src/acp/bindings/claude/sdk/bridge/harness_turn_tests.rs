//! Claude Code runs on its own after a background task ends: it reports the
//! task, then streams a run of the main agent that ends on a `result`, all
//! after the turn Sage prompted has ended.

use serde_json::{Value, json};

use super::tests::{SESSION, initialized, prompt, success};
use super::*;

fn finish_prompted_turn(bridge: &mut SdkBridge) {
    prompt(bridge, 3);
    let result = bridge
        .inbound(json!({"type":"result","subtype":"success","is_error":false,"usage":{}}))
        .unwrap();
    assert_eq!(result.acp[0]["id"], 3);
}

fn cancel(bridge: &mut SdkBridge) -> Value {
    bridge
        .outbound(
            &json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":SESSION}}),
        )
        .unwrap()
}

fn subagent_approval() -> Value {
    json!({"type":"control_request","request_id":"r7","request":{
        "subtype":"can_use_tool","tool_name":"Bash","tool_use_id":"toolu_sub",
        "input":{"command":"ls"}}})
}

fn run_start() -> Value {
    json!({"type":"stream_event","parent_tool_use_id":null,
        "event":{"type":"message_start","message":{"id":"msg_9"}}})
}

#[test]
fn a_run_claude_code_starts_on_its_own_opens_a_turn_that_its_result_ends() {
    let mut bridge = initialized();
    finish_prompted_turn(&mut bridge);

    let notified = bridge
        .inbound(
            json!({"type":"system","subtype":"task_notification","task_id":"a1",
            "status":"completed","output_file":"","summary":"Done"}),
        )
        .unwrap();
    assert!(!notified.started, "the notice alone opens no turn");
    let init = bridge
        .inbound(json!({"type":"system","subtype":"init","session_id":SESSION}))
        .unwrap();
    assert!(!init.started);

    assert!(bridge.inbound(run_start()).unwrap().started);
    bridge.adopt(json!(4)).unwrap();

    let chunk = bridge
        .inbound(json!({"type":"stream_event","parent_tool_use_id":null,
            "event":{"type":"content_block_delta","index":0,
                "delta":{"type":"text_delta","text":"The agent finished."}}}))
        .unwrap();
    assert!(!chunk.started);
    assert_eq!(
        chunk.acp[0]["params"]["update"],
        json!({"sessionUpdate":"agent_message_chunk","itemId":"msg_9:0",
            "content":{"type":"text","text":"The agent finished."}})
    );
    let result = bridge
        .inbound(json!({"type":"result","subtype":"success","is_error":false,
            "usage":{"input_tokens":5,"output_tokens":7}}))
        .unwrap();
    assert_eq!(result.acp[0]["id"], 4);
    assert_eq!(result.acp[0]["result"]["stopReason"], "end_turn");
    assert_eq!(result.acp[0]["result"]["usage"]["totalTokens"], 12);
}

#[test]
fn a_subagents_frames_alone_open_no_turn() {
    let mut bridge = initialized();
    finish_prompted_turn(&mut bridge);
    let nested = bridge
        .inbound(json!({"type":"assistant","parent_tool_use_id":"toolu_task",
            "message":{"id":"msg_sub","content":[{"type":"text","text":"Searching"}]}}))
        .unwrap();
    assert!(!nested.started);
}

#[test]
fn an_approval_asked_with_no_turn_open_opens_one_instead_of_being_declined() {
    let mut bridge = initialized();
    finish_prompted_turn(&mut bridge);
    let ask = bridge.inbound(subagent_approval()).unwrap();
    assert!(ask.started);
    assert!(ask.replies.is_empty(), "nothing is declined");
    assert!(
        ask.acp
            .iter()
            .any(|frame| frame["method"] == "session/request_permission")
    );
}

#[test]
fn stopping_a_turn_opened_only_for_an_approval_ends_it_and_declines_the_approval() {
    let mut bridge = initialized();
    finish_prompted_turn(&mut bridge);
    assert!(bridge.inbound(subagent_approval()).unwrap().started);
    bridge.adopt(json!(4)).unwrap();

    let interrupt = cancel(&mut bridge);
    assert_eq!(interrupt["request"]["subtype"], "interrupt");
    let stopped = bridge.inbound(success(&interrupt, json!({}))).unwrap();
    assert_eq!(stopped.withdrawn.len(), 1);
    assert_eq!(stopped.replies[0]["response"]["request_id"], "r7");
    assert_eq!(
        stopped.replies[0]["response"]["response"]["behavior"],
        "deny"
    );
    let ended = stopped
        .acp
        .iter()
        .find(|frame| frame["id"] == 4)
        .expect("the turn ends");
    assert_eq!(ended["result"]["stopReason"], "cancelled");
}

#[test]
fn stopping_a_running_harness_turn_waits_for_its_result() {
    let mut bridge = initialized();
    finish_prompted_turn(&mut bridge);
    assert!(bridge.inbound(run_start()).unwrap().started);
    bridge.adopt(json!(4)).unwrap();
    let interrupt = cancel(&mut bridge);
    let replied = bridge.inbound(success(&interrupt, json!({}))).unwrap();
    assert!(replied.acp.is_empty());
    let result = bridge
        .inbound(
            json!({"type":"result","subtype":"error_during_execution","is_error":true,"usage":{}}),
        )
        .unwrap();
    assert_eq!(result.acp[0]["id"], 4);
    assert_eq!(result.acp[0]["result"]["stopReason"], "cancelled");
}
