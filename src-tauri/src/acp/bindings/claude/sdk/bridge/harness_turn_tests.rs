//! Claude Code runs on its own after a background task ends: it reports the
//! task, then runs the main agent from a `system/init` to a `result`, all
//! after the turn Sage prompted has ended.

use serde_json::{Value, json};

use super::tests::{SESSION, initialized, prompt, success};
use super::*;

fn finish_prompted_turn(bridge: &mut SdkBridge) {
    prompt(bridge, 3);
    let result = bridge.inbound(result_frame("success")).unwrap();
    assert_eq!(result.acp[0]["id"], 3);
}

fn result_frame(subtype: &str) -> Value {
    json!({"type":"result","subtype":subtype,"is_error":subtype != "success","usage":{}})
}

fn cancel(bridge: &mut SdkBridge) -> Value {
    bridge
        .outbound(
            &json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":SESSION}}),
        )
        .unwrap()
}

fn subagent_approval(request: &str) -> Value {
    json!({"type":"control_request","request_id":request,"request":{
        "subtype":"can_use_tool","tool_name":"Bash","tool_use_id":format!("toolu_{request}"),
        "input":{"command":"ls"}}})
}

fn run_start() -> Value {
    json!({"type":"system","subtype":"init","session_id":SESSION})
}

fn text(chunk: &str) -> Value {
    json!({"type":"stream_event","parent_tool_use_id":null,
        "event":{"type":"content_block_delta","index":0,
            "delta":{"type":"text_delta","text":chunk}}})
}

fn ends(inbound: &Inbound, id: u64) -> Option<&Value> {
    inbound.acp.iter().find(|frame| frame["id"] == id)
}

fn denial(inbound: &Inbound, request: &str) -> bool {
    inbound.replies.iter().any(|reply| {
        reply["response"]["request_id"] == request
            && reply["response"]["response"]["behavior"] == "deny"
    })
}

fn allow(bridge: &mut SdkBridge, wire: u64) {
    let answer = bridge
        .outbound(
            &json!({"id":wire,"result":{"outcome":{"outcome":"selected","optionId":"allow_once"}}}),
        )
        .unwrap();
    let request = answer["response"]["request_id"]
        .as_str()
        .unwrap()
        .to_owned();
    bridge.delivered(&Sent::Answer(request));
}

fn wire(inbound: &Inbound) -> u64 {
    inbound
        .acp
        .iter()
        .find(|frame| frame["method"] == "session/request_permission")
        .and_then(|frame| frame["id"].as_u64())
        .expect("an approval card")
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

    assert!(bridge.inbound(run_start()).unwrap().started);
    bridge.adopt(json!(4)).unwrap();

    bridge
        .inbound(json!({"type":"stream_event","parent_tool_use_id":null,
            "event":{"type":"message_start","message":{"id":"msg_9"}}}))
        .unwrap();
    let chunk = bridge.inbound(text("The agent finished.")).unwrap();
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
    let ended = ends(&result, 4).expect("the run's result ends its turn");
    assert_eq!(ended["result"]["stopReason"], "end_turn");
    assert_eq!(ended["result"]["usage"]["totalTokens"], 12);
}

#[test]
fn nothing_opens_a_turn_before_sage_has_prompted() {
    let mut bridge = initialized();
    assert!(!bridge.inbound(run_start()).unwrap().started);
    let ask = bridge.inbound(subagent_approval("r1")).unwrap();
    assert!(!ask.started);
    assert!(denial(&ask, "r1"));
    assert!(ask.acp.is_empty());
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
fn stray_frames_after_a_stopped_turn_open_no_turn() {
    let mut bridge = initialized();
    prompt(&mut bridge, 3);
    let interrupt = cancel(&mut bridge);
    assert!(
        bridge
            .inbound(success(&interrupt, json!({})))
            .unwrap()
            .acp
            .is_empty()
    );
    let stopped = bridge
        .inbound(result_frame("error_during_execution"))
        .unwrap();
    assert_eq!(
        ends(&stopped, 3).unwrap()["result"]["stopReason"],
        "cancelled"
    );

    let interrupted = bridge
        .inbound(
            json!({"type":"user","parent_tool_use_id":null,"message":{"role":"user",
            "content":[{"type":"text","text":"[Request interrupted by user]"}]}}),
        )
        .unwrap();
    assert!(!interrupted.started);
    assert!(!bridge.inbound(text("late")).unwrap().started);
}

#[test]
fn stopping_while_a_queued_message_runs_waits_for_its_result_and_opens_no_other_turn() {
    let mut bridge = initialized();
    prompt(&mut bridge, 3);
    let (_, uuid) = bridge.steer("also this").unwrap();
    let lifecycle = |state: &str| json!({"type":"command_lifecycle","command_uuid":uuid,"state":state,"uuid":"e1","session_id":SESSION});
    bridge.inbound(lifecycle("queued")).unwrap();
    let held = bridge.inbound(result_frame("success")).unwrap();
    assert!(ends(&held, 3).is_none(), "the queued message runs next");
    let withdraw = cancel(&mut bridge);
    bridge.inbound(lifecycle("started")).unwrap();
    let reply = bridge
        .inbound(success(&withdraw, json!({"cancelled":false})))
        .unwrap();
    let interrupt = &reply.replies[0];
    assert_eq!(interrupt["request"]["subtype"], "interrupt");
    let answered = bridge.inbound(success(interrupt, json!({}))).unwrap();
    assert!(ends(&answered, 3).is_none(), "the stop waits for the run");

    assert!(!bridge.inbound(run_start()).unwrap().started);
    assert!(!bridge.inbound(text("Partly done")).unwrap().started);
    let end = bridge
        .inbound(result_frame("error_during_execution"))
        .unwrap();
    assert_eq!(ends(&end, 3).unwrap()["result"]["stopReason"], "cancelled");
}

#[test]
fn an_approval_asked_with_no_turn_open_opens_one_instead_of_being_declined() {
    let mut bridge = initialized();
    finish_prompted_turn(&mut bridge);
    let ask = bridge.inbound(subagent_approval("r7")).unwrap();
    assert!(ask.started);
    assert!(ask.replies.is_empty(), "nothing is declined");
    wire(&ask);
}

#[test]
fn an_approval_declined_for_too_many_waiting_opens_no_turn() {
    let mut bridge = initialized();
    prompt(&mut bridge, 3);
    for request in 0..OPEN_APPROVALS {
        let asked = bridge
            .inbound(subagent_approval(&format!("r{request}")))
            .unwrap();
        bridge
            .outbound(&json!({"id":wire(&asked),"result":{"outcome":{"outcome":"selected","optionId":"allow_once"}}}))
            .unwrap();
    }
    bridge.inbound(result_frame("success")).unwrap();
    let over = bridge.inbound(subagent_approval("over")).unwrap();
    assert!(denial(&over, "over"));
    assert!(!over.started);
}

#[test]
fn a_turn_opened_for_an_approval_ends_once_it_settles_and_a_later_run_opens_its_own() {
    let mut bridge = initialized();
    finish_prompted_turn(&mut bridge);
    let ask = bridge.inbound(subagent_approval("r7")).unwrap();
    assert!(ask.started);
    bridge.adopt(json!(4)).unwrap();
    allow(&mut bridge, wire(&ask));

    let progress = bridge
        .inbound(
            json!({"type":"system","subtype":"task_progress","task_id":"a1",
            "description":"Background agent","last_tool_name":"Bash"}),
        )
        .unwrap();
    let prior = progress.prior.as_deref().expect("the settled turn ends");
    assert_eq!(ends(prior, 4).unwrap()["result"]["stopReason"], "end_turn");
    assert!(!progress.started);

    let run = bridge.inbound(run_start()).unwrap();
    assert!(run.prior.is_none());
    assert!(run.started, "the report's run opens a turn of its own");
}

#[test]
fn a_run_starting_as_an_approval_settles_ends_that_turn_first() {
    let mut bridge = initialized();
    finish_prompted_turn(&mut bridge);
    let ask = bridge.inbound(subagent_approval("r7")).unwrap();
    bridge.adopt(json!(4)).unwrap();
    allow(&mut bridge, wire(&ask));
    let run = bridge.inbound(run_start()).unwrap();
    assert!(ends(run.prior.as_deref().unwrap(), 4).is_some());
    assert!(run.started);
}

#[test]
fn a_run_that_starts_while_an_approval_waits_joins_its_turn() {
    let mut bridge = initialized();
    finish_prompted_turn(&mut bridge);
    let ask = bridge.inbound(subagent_approval("r7")).unwrap();
    bridge.adopt(json!(4)).unwrap();
    let run = bridge.inbound(run_start()).unwrap();
    assert!(!run.started && run.prior.is_none());
    allow(&mut bridge, wire(&ask));
    assert!(bridge.inbound(text("Working")).unwrap().prior.is_none());
    let end = bridge.inbound(result_frame("success")).unwrap();
    assert!(ends(&end, 4).is_some(), "the run's result ends the turn");
}

#[test]
fn stopping_a_turn_opened_only_for_an_approval_ends_it_and_declines_the_approval() {
    let mut bridge = initialized();
    finish_prompted_turn(&mut bridge);
    assert!(bridge.inbound(subagent_approval("r7")).unwrap().started);
    bridge.adopt(json!(4)).unwrap();

    let interrupt = cancel(&mut bridge);
    assert_eq!(interrupt["request"]["subtype"], "interrupt");
    let stopped = bridge.inbound(success(&interrupt, json!({}))).unwrap();
    assert_eq!(stopped.withdrawn.len(), 1);
    assert!(denial(&stopped, "r7"));
    assert_eq!(
        ends(&stopped, 4).unwrap()["result"]["stopReason"],
        "cancelled"
    );
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
        .inbound(result_frame("error_during_execution"))
        .unwrap();
    assert_eq!(
        ends(&result, 4).unwrap()["result"]["stopReason"],
        "cancelled"
    );
}

#[test]
fn a_declined_run_is_interrupted_and_read_to_its_end_unseen() {
    let mut bridge = initialized();
    finish_prompted_turn(&mut bridge);
    assert!(bridge.inbound(run_start()).unwrap().started);
    let declined = bridge.decline().unwrap();
    let interrupt = &declined.replies[0];
    assert_eq!(interrupt["request"]["subtype"], "interrupt");

    assert!(bridge.inbound(text("Hidden")).unwrap().acp.is_empty());
    let ask = bridge.inbound(subagent_approval("r8")).unwrap();
    assert!(denial(&ask, "r8") && !ask.started && ask.acp.is_empty());
    assert!(
        bridge
            .inbound(success(interrupt, json!({})))
            .unwrap()
            .acp
            .is_empty()
    );
    let end = bridge
        .inbound(result_frame("error_during_execution"))
        .unwrap();
    assert!(end.acp.is_empty());

    assert!(
        bridge.inbound(run_start()).unwrap().started,
        "the next run opens a turn"
    );
}

#[test]
fn a_declined_approval_is_denied_without_an_interrupt() {
    let mut bridge = initialized();
    finish_prompted_turn(&mut bridge);
    assert!(bridge.inbound(subagent_approval("r7")).unwrap().started);
    let declined = bridge.decline().unwrap();
    assert_eq!(declined.replies.len(), 1);
    assert!(denial(&declined, "r7"));
    assert!(bridge.inbound(subagent_approval("r8")).unwrap().started);
}
