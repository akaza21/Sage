//! The SDK replay journeys, driven through the bridge as a live session
//! would drive them: each journey must complete and render its turn.

use serde_json::{Value, json};

use super::bridge::SdkBridge;

const SESSION: &str = "0b7d6c8e-3c1a-4f7e-9a51-2f6d8c1e4b90";

/// Replays `case` and returns every ACP frame the bridge produced. The
/// client approves the first permission option it is asked about.
fn replay(case: &str) -> Vec<Value> {
    let mut bridge = SdkBridge::new(SESSION.into(), None);
    let mut acp = Vec::new();
    crate::acp::fixture::claude_sdk_wire_for_test(case, |kind, frames| {
        for frame in frames {
            let inbound = bridge.inbound(frame).expect("SDK frame maps");
            assert!(inbound.replies.is_empty(), "{case}: no unserved requests");
            acp.extend(inbound.acp);
        }
        let request = match kind {
            Some("claudeSdkInitialize") => json!({"id": 1, "method": "initialize", "params": {}}),
            Some("claudeSdkSettings") => json!({"id": 2, "method": "session/new", "params": {}}),
            Some("claudeSdkPrompt") => json!({"id": 3, "method": "session/prompt", "params": {
                "sessionId": SESSION, "prompt": [{"type": "text", "text": "Inspect"}],
            }}),
            Some("claudeSdkInterrupt") => json!({"method": "session/cancel", "params": {}}),
            Some("claudeSdkPermission") => {
                let asked = acp
                    .iter()
                    .rfind(|frame| frame["method"] == "session/request_permission")
                    .expect("permission asked");
                json!({"id": asked["id"], "result": {"outcome": {
                    "outcome": "selected", "optionId": asked["params"]["options"][0]["optionId"],
                }}})
            }
            _ => return Vec::new(),
        };
        vec![bridge.outbound(&request).expect("ACP request maps")]
    });
    acp
}

fn updates<'a>(acp: &'a [Value], kind: &str) -> Vec<&'a Value> {
    acp.iter()
        .map(|frame| &frame["params"]["update"])
        .filter(|update| update["sessionUpdate"] == kind)
        .collect()
}

fn prompt_reply(acp: &[Value]) -> &Value {
    acp.iter()
        .find(|frame| frame["id"] == 3)
        .expect("prompt settles")
}

#[test]
fn a_completed_turn_streams_thinking_and_text_once() {
    let acp = replay("claude-sdk-turn-completed");
    assert_eq!(updates(&acp, "agent_thought_chunk").len(), 1);
    assert_eq!(updates(&acp, "agent_message_chunk").len(), 1);
    assert_eq!(updates(&acp, "usage_update")[0]["size"], 200_000);
    assert_eq!(prompt_reply(&acp)["result"]["stopReason"], "end_turn");
}

#[test]
fn an_edit_renders_its_diff_and_completes() {
    let acp = replay("claude-sdk-file-edit");
    let calls = updates(&acp, "tool_call");
    assert_eq!(
        calls.iter().map(|call| &call["kind"]).collect::<Vec<_>>(),
        ["read", "edit"]
    );
    let diff = &calls[1]["content"][0];
    assert_eq!(diff["type"], "diff");
    assert!(diff["path"].as_str().unwrap().ends_with("example.ts"));
    assert_eq!(diff["newText"], "export const example = 2;");
    let done = updates(&acp, "tool_call_update");
    assert!(done.iter().all(|update| update["status"] == "completed"));
}

#[test]
fn an_approved_tool_runs_after_the_permission_round_trip() {
    let acp = replay("claude-sdk-tool-approval");
    assert!(
        acp.iter()
            .any(|frame| frame["method"] == "session/request_permission")
    );
    assert_eq!(updates(&acp, "tool_call")[0]["title"], "git status --short");
    assert_eq!(updates(&acp, "tool_call_update")[0]["status"], "completed");
    assert_eq!(prompt_reply(&acp)["result"]["stopReason"], "end_turn");
}

#[test]
fn todos_become_the_plan_without_a_tool_row() {
    let acp = replay("claude-sdk-plan");
    assert_eq!(
        updates(&acp, "plan")[0]["entries"][1]["content"],
        "Updating the synthetic example"
    );
    assert!(updates(&acp, "tool_call").is_empty());
}

#[test]
fn stopped_failed_and_signed_out_turns_end_as_the_other_harnesses_do() {
    let stopped = replay("claude-sdk-turn-stopped");
    assert_eq!(prompt_reply(&stopped)["result"]["stopReason"], "cancelled");
    let failed = replay("claude-sdk-turn-error");
    assert!(
        prompt_reply(&failed)["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Overloaded")
    );
    let signed_out = replay("claude-sdk-sign-in");
    assert_eq!(prompt_reply(&signed_out)["error"]["code"], -32_000);
}
