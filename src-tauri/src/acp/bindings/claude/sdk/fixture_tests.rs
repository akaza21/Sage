//! The SDK replay journeys, driven through the bridge as a live session
//! would drive them: each journey must complete and render its turn.

use serde_json::{Value, json};

use super::bridge::SdkBridge;
use crate::acp::intent::session_event::SessionScopedEvent;

const SESSION: &str = "0b7d6c8e-3c1a-4f7e-9a51-2f6d8c1e4b90";

/// What a replay produced: every ACP frame, each mid-turn message's
/// answer, and the text of each session notice.
struct Replay {
    acp: Vec<Value>,
    steers: Vec<(String, bool)>,
    notices: Vec<String>,
}

/// Replays `case` and returns what the bridge produced. Prompts are sent
/// in order with ids from 3; a message sent while Claude works says "Only
/// the first page". The client approves the first permission option it is
/// asked about.
fn replay_with(case: &str, prompts: &[Value]) -> Replay {
    let mut bridge = SdkBridge::new(SESSION.into(), None);
    let mut acp = Vec::new();
    let mut steers = Vec::new();
    let mut notices = Vec::new();
    let mut prompts = prompts.iter().zip(3..);
    crate::acp::fixture::claude_sdk_wire_for_test(case, |kind, frames| {
        let mut sent = Vec::new();
        for frame in frames {
            let inbound = bridge.inbound(frame).expect("SDK frame maps");
            // Requests the bridge makes on its own, such as the interrupt
            // that follows a withdrawn message.
            sent.extend(inbound.replies);
            steers.extend(inbound.steers);
            notices.extend(inbound.session.into_iter().filter_map(|event| match event {
                SessionScopedEvent::Notice(notice) => Some(notice.text.as_str().to_owned()),
                _ => None,
            }));
            acp.extend(inbound.acp);
        }
        let request = match kind {
            Some("claudeSdkInitialize") => json!({"id": 1, "method": "initialize", "params": {}}),
            Some("claudeSdkSettings") => json!({"id": 2, "method": "session/new", "params": {}}),
            Some("claudeSdkPrompt") => {
                let (prompt, id) = prompts.next().expect("a prompt to send");
                json!({"id": id, "method": "session/prompt", "params": {
                    "sessionId": SESSION, "prompt": prompt,
                }})
            }
            Some("claudeSdkFollowUp") => {
                sent.push(bridge.steer("Only the first page").expect("steer").0);
                return sent;
            }
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
            _ => return sent,
        };
        sent.push(bridge.outbound(&request).expect("ACP request maps"));
        sent
    });
    Replay {
        acp,
        steers,
        notices,
    }
}

fn replay(case: &str) -> Vec<Value> {
    replay_with(case, &[text("Inspect")]).acp
}

fn text(text: &str) -> Value {
    json!([{"type": "text", "text": text}])
}

/// The text Claude streamed, in order.
fn said(acp: &[Value]) -> String {
    updates(acp, "agent_message_chunk")
        .iter()
        .filter_map(|update| update["content"]["text"].as_str())
        .collect()
}

fn settled(acp: &[Value], id: u64) -> &Value {
    acp.iter()
        .find(|frame| frame["id"] == id && frame.get("method").is_none())
        .expect("prompt settles")
}

fn updates<'a>(acp: &'a [Value], kind: &str) -> Vec<&'a Value> {
    acp.iter()
        .map(|frame| &frame["params"]["update"])
        .filter(|update| update["sessionUpdate"] == kind)
        .collect()
}

fn prompt_reply(acp: &[Value]) -> &Value {
    settled(acp, 3)
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

#[test]
fn a_message_sent_during_a_tool_runs_in_the_same_turn() {
    let Replay { acp, steers, .. } = replay_with("claude-sdk-follow-up-tool", &[text("Inspect")]);
    assert!(
        matches!(&steers[..], [(_, true), ..]),
        "queued and accepted"
    );
    assert!(
        said(&acp).contains("Only the first page"),
        "Claude answers it"
    );
    assert_eq!(updates(&acp, "tool_call_update")[0]["status"], "completed");
    assert_eq!(prompt_reply(&acp)["result"]["stopReason"], "end_turn");
}

#[test]
fn a_message_sent_during_a_reply_runs_after_it_before_the_turn_ends() {
    let Replay { acp, steers, .. } = replay_with("claude-sdk-follow-up-reply", &[text("Inspect")]);
    assert!(matches!(&steers[..], [(_, true), ..]));
    let said = said(&acp);
    let reply = said.find("still being read.").expect("the reply streams");
    let answer = said
        .find("Only the first page")
        .expect("then the message runs");
    assert!(reply < answer);
    let ends = acp.iter().filter(|frame| frame["id"] == 3).count();
    assert_eq!(ends, 1, "one turn, ended once");
    assert_eq!(prompt_reply(&acp)["result"]["stopReason"], "end_turn");
}

#[test]
fn stop_withdraws_a_waiting_message_and_cancels_the_turn() {
    let Replay { acp, steers, .. } = replay_with("claude-sdk-follow-up-stop", &[text("Inspect")]);
    let uuid = &steers[0].0;
    assert_eq!(steers, [(uuid.clone(), true), (uuid.clone(), false)]);
    assert!(!said(&acp).contains("Only the first page"), "it never ran");
    assert_eq!(prompt_reply(&acp)["result"]["stopReason"], "cancelled");
}

#[test]
fn compact_after_an_answer_ends_its_own_turn_and_keeps_the_summary_private() {
    let Replay { acp, notices, .. } =
        replay_with("claude-sdk-compact", &[text("Inspect"), text("/compact")]);
    assert_eq!(settled(&acp, 3)["result"]["stopReason"], "end_turn");
    assert_eq!(settled(&acp, 4)["result"]["stopReason"], "end_turn");
    assert_eq!(notices, ["Claude Code compacted the conversation."]);
    let shown = serde_json::to_string(&acp).unwrap() + &notices.concat();
    assert!(!shown.contains("the synthetic example was reviewed"));
}

#[test]
fn compact_in_a_new_chat_says_there_is_nothing_to_compact() {
    let acp = replay_with("claude-sdk-compact-empty", &[text("/compact")]).acp;
    assert_eq!(said(&acp), "Error: No messages to compact");
    assert_eq!(prompt_reply(&acp)["result"]["stopReason"], "end_turn");
}

#[test]
fn each_prompt_is_echoed_as_claude_code_received_it() {
    let link = |name: &str| {
        let uri = url::Url::from_file_path(std::env::temp_dir().join(name)).unwrap();
        json!([{"type": "text", "text": "Read this"},
            {"type": "resource_link", "uri": uri.as_str(), "name": name}])
    };
    let acp = replay_with("claude-sdk-prompt-echo", &[link(".env"), link("large.log")]).acp;
    let said = said(&acp);
    let (first, second) = said.split_at(said.rfind("Synthetic replay received").unwrap());
    assert!(first.contains("(Not attached: Sage does not send credential files.)"));
    let large = std::env::temp_dir().join("large.log");
    assert!(second.contains(&format!("@{}", large.display())));
    assert_eq!(settled(&acp, 4)["result"]["stopReason"], "end_turn");
}
