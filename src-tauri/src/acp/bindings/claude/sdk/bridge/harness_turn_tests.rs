//! Claude Code runs on its own after a background task ends: it reports the
//! task, then runs the main agent from a `system/init` to a `result`, all
//! after the turn Sage prompted has ended.

use serde_json::{Value, json};

use super::tests::{SESSION, initialized, prompt, success};
use super::*;

/// Sage's first prompt runs and ends, reported by Claude Code's lifecycle
/// frames as every stamped prompt is.
fn finish_prompted_turn(bridge: &mut SdkBridge) {
    const FIRST: &str = "0b7c4ad2-5f0e-4c1e-9d3a-2a6f8e1c7b90";
    bridge
        .outbound(
            &json!({"jsonrpc":"2.0","id":3,"method":"session/prompt","params":{
            "sessionId":SESSION,"messageId":format!("turn-{FIRST}"),
            "prompt":[{"type":"text","text":"start a task"}]}}),
        )
        .unwrap();
    let report = |state: &str| {
        json!({"type":"command_lifecycle","command_uuid":FIRST,
        "state":state,"uuid":"e0","session_id":SESSION})
    };
    bridge.inbound(report("queued")).unwrap();
    bridge.inbound(report("started")).unwrap();
    let result = bridge.inbound(result_frame("success")).unwrap();
    assert_eq!(result.acp[0]["id"], 3);
    assert!(bridge.inbound(report("completed")).unwrap().acp.is_empty());
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
    let started = bridge.inbound(lifecycle("started")).unwrap();
    let interrupt = &started.replies[0];
    assert_eq!(interrupt["request"]["subtype"], "interrupt");
    let reply = bridge
        .inbound(success(&withdraw, json!({"cancelled":false})))
        .unwrap();
    assert!(reply.replies.is_empty(), "one interrupt is enough");
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
fn a_turn_opened_for_an_approval_ends_once_its_answer_is_delivered() {
    let mut bridge = initialized();
    finish_prompted_turn(&mut bridge);
    let ask = bridge.inbound(subagent_approval("r7")).unwrap();
    assert!(ask.started);
    bridge.adopt(json!(4)).unwrap();
    let answer = bridge
        .outbound(&json!({"id":wire(&ask),"result":{"outcome":{"outcome":"selected","optionId":"allow_once"}}}))
        .unwrap();
    assert!(
        bridge.settled_turn_end().is_none(),
        "the answer has not reached Claude Code yet"
    );
    let request = answer["response"]["request_id"]
        .as_str()
        .unwrap()
        .to_owned();
    bridge.delivered(&Sent::Answer(request));
    let ended = bridge.settled_turn_end().expect("the settled turn ends");
    assert_eq!(ends(&ended, 4).unwrap()["result"]["stopReason"], "end_turn");

    let run = bridge.inbound(run_start()).unwrap();
    assert!(run.started, "the report's run opens a turn of its own");
}

#[test]
fn a_turn_opened_for_an_approval_claude_code_withdraws_ends_with_the_withdrawal() {
    let mut bridge = initialized();
    finish_prompted_turn(&mut bridge);
    assert!(bridge.inbound(subagent_approval("r7")).unwrap().started);
    bridge.adopt(json!(4)).unwrap();
    let withdrawn = bridge
        .inbound(json!({"type":"control_cancel_request","request_id":"r7"}))
        .unwrap();
    assert_eq!(withdrawn.withdrawn.len(), 1);
    let ended = bridge
        .settled_turn_end()
        .expect("nothing is left to wait on");
    assert_eq!(ends(&ended, 4).unwrap()["result"]["stopReason"], "end_turn");
}

#[test]
fn a_run_that_starts_while_an_approval_waits_joins_its_turn() {
    let mut bridge = initialized();
    finish_prompted_turn(&mut bridge);
    let ask = bridge.inbound(subagent_approval("r7")).unwrap();
    bridge.adopt(json!(4)).unwrap();
    assert!(!bridge.inbound(run_start()).unwrap().started);
    allow(&mut bridge, wire(&ask));
    assert!(bridge.settled_turn_end().is_none(), "the run in it goes on");
    assert!(ends(&bridge.inbound(text("Working")).unwrap(), 4).is_none());
    assert!(bridge.settled_turn_end().is_none());
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

const PROMPT_UUID: &str = "6f9619ff-8b86-4011-b42d-00cf4fc964ff";

fn prompt_with_uuid(bridge: &mut SdkBridge, id: u64) -> Value {
    let user = bridge
        .outbound(
            &json!({"jsonrpc":"2.0","id":id,"method":"session/prompt","params":{
            "sessionId":SESSION,"messageId":format!("turn-{PROMPT_UUID}"),
            "prompt":[{"type":"text","text":"hi"}]}}),
        )
        .unwrap();
    assert_eq!(user["uuid"], PROMPT_UUID);
    user
}

fn prompt_lifecycle(state: &str) -> Value {
    json!({"type":"command_lifecycle","command_uuid":PROMPT_UUID,"state":state,
        "uuid":"e1","session_id":SESSION})
}

#[test]
fn a_run_claude_code_started_before_taking_up_the_prompt_leaves_the_turn_to_the_prompts_run() {
    let mut bridge = initialized();
    finish_prompted_turn(&mut bridge);
    prompt_with_uuid(&mut bridge, 5);

    assert!(!bridge.inbound(run_start()).unwrap().started);
    assert!(
        !bridge
            .inbound(text("Background report"))
            .unwrap()
            .acp
            .is_empty()
    );
    bridge.inbound(prompt_lifecycle("queued")).unwrap();
    let held = bridge.inbound(result_frame("success")).unwrap();
    assert!(ends(&held, 5).is_none(), "the prompt has not run yet");

    bridge.inbound(prompt_lifecycle("started")).unwrap();
    assert!(!bridge.inbound(run_start()).unwrap().started);
    bridge.inbound(text("The reply")).unwrap();
    let end = bridge.inbound(result_frame("success")).unwrap();
    assert_eq!(ends(&end, 5).unwrap()["result"]["stopReason"], "end_turn");
    bridge.inbound(prompt_lifecycle("completed")).unwrap();
    assert!(
        bridge.inbound(run_start()).unwrap().started,
        "a later run opens a turn of its own"
    );
}

#[test]
fn a_run_that_ended_before_claude_code_read_the_prompt_does_not_end_its_turn() {
    let mut bridge = initialized();
    finish_prompted_turn(&mut bridge);
    prompt_with_uuid(&mut bridge, 5);
    bridge.inbound(run_start()).unwrap();
    let held = bridge.inbound(result_frame("success")).unwrap();
    assert!(ends(&held, 5).is_none());

    bridge.inbound(prompt_lifecycle("queued")).unwrap();
    bridge.inbound(prompt_lifecycle("started")).unwrap();
    let run = bridge.inbound(run_start()).unwrap();
    assert!(
        !run.started,
        "the prompt's run is its turn's, not a new one"
    );
    let end = bridge.inbound(result_frame("success")).unwrap();
    assert!(ends(&end, 5).is_some());
}

#[test]
fn a_prompt_folded_into_the_running_run_ends_with_that_runs_result() {
    let mut bridge = initialized();
    finish_prompted_turn(&mut bridge);
    prompt_with_uuid(&mut bridge, 5);
    bridge.inbound(run_start()).unwrap();
    bridge.inbound(prompt_lifecycle("queued")).unwrap();
    bridge.inbound(prompt_lifecycle("started")).unwrap();
    let end = bridge.inbound(result_frame("success")).unwrap();
    assert!(ends(&end, 5).is_some());
}

#[test]
fn a_prompt_claude_code_drops_after_a_run_ends_its_turn_with_an_error() {
    let mut bridge = initialized();
    finish_prompted_turn(&mut bridge);
    prompt_with_uuid(&mut bridge, 5);
    bridge.inbound(run_start()).unwrap();
    bridge.inbound(prompt_lifecycle("queued")).unwrap();
    assert!(ends(&bridge.inbound(result_frame("success")).unwrap(), 5).is_none());
    let dropped = bridge.inbound(prompt_lifecycle("discarded")).unwrap();
    assert_eq!(
        ends(&dropped, 5).unwrap()["error"]["message"],
        "Claude Code ended the session before running this message."
    );
}

#[test]
fn a_declined_run_that_takes_up_the_prompt_ends_the_prompts_turn() {
    let mut bridge = initialized();
    finish_prompted_turn(&mut bridge);
    assert!(bridge.inbound(run_start()).unwrap().started);
    bridge.decline().unwrap();
    prompt_with_uuid(&mut bridge, 5);
    assert!(bridge.inbound(text("Hidden")).unwrap().acp.is_empty());

    bridge.inbound(prompt_lifecycle("queued")).unwrap();
    bridge.inbound(prompt_lifecycle("started")).unwrap();
    assert!(!bridge.inbound(text("The reply")).unwrap().acp.is_empty());
    let end = bridge
        .inbound(result_frame("error_during_execution"))
        .unwrap();
    assert!(ends(&end, 5).is_some(), "the prompt's turn gets its result");
}

fn withdrawal(bridge: &mut SdkBridge) -> Value {
    let withdraw = cancel(bridge);
    assert_eq!(
        withdraw["request"],
        json!({"subtype":"cancel_async_message","message_uuid":PROMPT_UUID})
    );
    withdraw
}

#[test]
fn a_stop_before_claude_code_takes_up_the_prompt_withdraws_it_and_stops_the_run() {
    let mut bridge = initialized();
    finish_prompted_turn(&mut bridge);
    prompt_with_uuid(&mut bridge, 5);
    bridge.inbound(run_start()).unwrap();
    bridge.inbound(prompt_lifecycle("queued")).unwrap();
    let withdraw = withdrawal(&mut bridge);
    let withdrawn = bridge
        .inbound(success(&withdraw, json!({"cancelled":true})))
        .unwrap();
    assert!(
        ends(&withdrawn, 5).is_none(),
        "the run still reports its end"
    );
    let interrupt = &withdrawn.replies[0];
    assert_eq!(interrupt["request"]["subtype"], "interrupt");
    bridge.inbound(success(interrupt, json!({}))).unwrap();
    let stopped = bridge
        .inbound(result_frame("error_during_execution"))
        .unwrap();
    assert_eq!(
        ends(&stopped, 5).unwrap()["result"]["stopReason"],
        "cancelled"
    );
    assert!(
        bridge
            .inbound(prompt_lifecycle("cancelled"))
            .unwrap()
            .acp
            .is_empty()
    );
}

#[test]
fn a_prompt_taken_up_before_its_withdrawal_is_stopped_by_the_interrupt() {
    let mut bridge = initialized();
    finish_prompted_turn(&mut bridge);
    prompt_with_uuid(&mut bridge, 5);
    bridge.inbound(run_start()).unwrap();
    bridge.inbound(prompt_lifecycle("queued")).unwrap();
    let withdraw = withdrawal(&mut bridge);
    let refused = bridge
        .inbound(success(&withdraw, json!({"cancelled":false})))
        .unwrap();
    let interrupt = &refused.replies[0];
    assert_eq!(interrupt["request"]["subtype"], "interrupt");
    bridge.inbound(success(interrupt, json!({}))).unwrap();
    let stopped = bridge
        .inbound(result_frame("error_during_execution"))
        .unwrap();
    assert!(ends(&stopped, 5).is_none(), "the prompt still runs");

    let started = bridge.inbound(prompt_lifecycle("started")).unwrap();
    assert_eq!(started.replies[0]["request"]["subtype"], "interrupt");
    let end = bridge
        .inbound(result_frame("error_during_execution"))
        .unwrap();
    assert_eq!(ends(&end, 5).unwrap()["result"]["stopReason"], "cancelled");
}

fn requeuing_run_start() -> Value {
    json!({"type":"system","subtype":"init","session_id":SESSION,
        "capabilities":["interrupt_receipt_v1","interrupt_requeue_taken_v1"]})
}

/// Sage declines a run from a build that can queue a prompt again, then the
/// user prompts while its interrupt is on the way.
fn prompt_while_declining(bridge: &mut SdkBridge) -> (Value, Value) {
    finish_prompted_turn(bridge);
    assert!(bridge.inbound(requeuing_run_start()).unwrap().started);
    let interrupt = bridge.decline().unwrap().replies[0].clone();
    assert_eq!(interrupt["request"]["subtype"], "interrupt");
    let user = prompt_with_uuid(bridge, 5);
    bridge.inbound(prompt_lifecycle("queued")).unwrap();
    (interrupt, user)
}

#[test]
fn a_prompt_the_declined_run_takes_in_runs_again_after_sages_interrupt_stops_it() {
    let mut bridge = initialized();
    let (interrupt, user) = prompt_while_declining(&mut bridge);
    assert_eq!(user["requeue_if_interrupted_within_ms"], 10_000);

    bridge.inbound(prompt_lifecycle("started")).unwrap();
    assert!(bridge.inbound(text("Cut short")).unwrap().acp.is_empty());
    bridge
        .inbound(success(
            &interrupt,
            json!({"still_queued":[PROMPT_UUID],"put_back":[PROMPT_UUID]}),
        ))
        .unwrap();
    let stopped = bridge
        .inbound(result_frame("error_during_execution"))
        .unwrap();
    assert!(
        stopped.acp.is_empty(),
        "the stopped run was Sage's to decline"
    );

    bridge.inbound(prompt_lifecycle("queued")).unwrap();
    bridge.inbound(prompt_lifecycle("started")).unwrap();
    assert!(!bridge.inbound(run_start()).unwrap().started);
    assert!(!bridge.inbound(text("The reply")).unwrap().acp.is_empty());
    let end = bridge.inbound(result_frame("success")).unwrap();
    assert_eq!(ends(&end, 5).unwrap()["result"]["stopReason"], "end_turn");
}

#[test]
fn a_prompt_the_declined_run_takes_in_but_cannot_give_back_ends_with_an_error() {
    let mut bridge = initialized();
    prompt_while_declining(&mut bridge);
    bridge.inbound(prompt_lifecycle("started")).unwrap();
    let lost = bridge.inbound(prompt_lifecycle("cancelled")).unwrap();
    assert_eq!(
        ends(&lost, 5).unwrap()["error"]["message"],
        "Claude Code cancelled this message before answering it."
    );
    assert!(
        bridge
            .inbound(result_frame("error_during_execution"))
            .unwrap()
            .acp
            .is_empty()
    );
}

#[test]
fn stopping_a_prompt_sage_stamped_withdraws_it_so_no_interrupt_puts_it_back() {
    let mut bridge = initialized();
    prompt_while_declining(&mut bridge);
    let withdraw = withdrawal(&mut bridge);
    let ended = bridge
        .inbound(success(&withdraw, json!({"cancelled":true})))
        .unwrap();
    assert!(
        ended.replies.is_empty(),
        "Sage's interrupt already stops the declined run"
    );
    assert_eq!(
        ends(&ended, 5).unwrap()["result"]["stopReason"],
        "cancelled"
    );
    assert!(
        bridge
            .inbound(prompt_lifecycle("cancelled"))
            .unwrap()
            .acp
            .is_empty()
    );
}

#[test]
fn a_prompt_sent_with_no_run_declining_is_not_stamped() {
    let mut bridge = initialized();
    finish_prompted_turn(&mut bridge);
    bridge.inbound(requeuing_run_start()).unwrap();
    bridge.adopt(json!(4)).unwrap();
    bridge.inbound(result_frame("success")).unwrap();
    let user = prompt_with_uuid(&mut bridge, 5);
    assert!(user.get("requeue_if_interrupted_within_ms").is_none());
    withdrawal(&mut bridge);
}

#[test]
fn a_prompt_claude_code_refuses_before_any_run_ends_its_turn_with_an_error() {
    let mut bridge = initialized();
    finish_prompted_turn(&mut bridge);
    prompt_with_uuid(&mut bridge, 5);
    let refused = bridge.inbound(prompt_lifecycle("refused")).unwrap();
    assert_eq!(
        ends(&refused, 5).unwrap()["error"]["message"],
        "Claude Code refused this message."
    );
    assert!(bridge.inbound(run_start()).unwrap().started);
}

#[test]
fn a_prompt_stopped_before_any_run_ends_its_turn_cancelled() {
    let mut bridge = initialized();
    finish_prompted_turn(&mut bridge);
    prompt_with_uuid(&mut bridge, 5);
    bridge.inbound(prompt_lifecycle("queued")).unwrap();
    let withdraw = withdrawal(&mut bridge);
    let stopped = bridge
        .inbound(success(&withdraw, json!({"cancelled":true})))
        .unwrap();
    assert!(stopped.replies.is_empty(), "no run is left to interrupt");
    assert_eq!(
        ends(&stopped, 5).unwrap()["result"]["stopReason"],
        "cancelled"
    );
}

#[test]
fn a_prompt_refused_while_a_run_goes_on_ends_with_the_refusal_once_the_run_ends() {
    let mut bridge = initialized();
    finish_prompted_turn(&mut bridge);
    prompt_with_uuid(&mut bridge, 5);
    bridge.inbound(run_start()).unwrap();
    let refused = bridge.inbound(prompt_lifecycle("refused")).unwrap();
    assert!(ends(&refused, 5).is_none(), "the run still reports its end");
    assert!(!bridge.inbound(text("Still going")).unwrap().started);
    let end = bridge.inbound(result_frame("success")).unwrap();
    assert_eq!(
        ends(&end, 5).unwrap()["error"]["message"],
        "Claude Code refused this message."
    );
}

#[test]
fn a_build_that_reports_no_lifecycle_ends_each_turn_on_its_result() {
    let mut bridge = initialized();
    prompt(&mut bridge, 3);
    assert!(ends(&bridge.inbound(result_frame("success")).unwrap(), 3).is_some());
    prompt_with_uuid(&mut bridge, 5);
    bridge.inbound(run_start()).unwrap();
    let end = bridge.inbound(result_frame("success")).unwrap();
    assert!(
        ends(&end, 5).is_some(),
        "no frame would ever say the prompt ran"
    );
}

#[test]
fn a_refusal_while_a_run_goes_on_still_reports_the_runs_context_window() {
    let mut bridge = initialized();
    finish_prompted_turn(&mut bridge);
    prompt_with_uuid(&mut bridge, 5);
    bridge.inbound(run_start()).unwrap();
    bridge.inbound(prompt_lifecycle("refused")).unwrap();
    let end = bridge
        .inbound(json!({"type":"result","subtype":"success","is_error":false,
            "usage":{"input_tokens":7},
            "modelUsage":{"claude-sonnet-5-5":{"contextWindow":200000}}}))
        .unwrap();
    assert_eq!(
        ends(&end, 5).unwrap()["error"]["message"],
        "Claude Code refused this message."
    );
    let usage = end
        .acp
        .iter()
        .find(|frame| frame["params"]["update"]["sessionUpdate"] == "usage_update")
        .expect("the ring still updates");
    assert_eq!(usage["params"]["update"]["size"], 200_000);
}

#[test]
fn a_stop_with_nothing_running_ends_once_the_prompt_and_messages_are_withdrawn() {
    let mut bridge = initialized();
    finish_prompted_turn(&mut bridge);
    prompt_with_uuid(&mut bridge, 5);
    bridge.inbound(prompt_lifecycle("queued")).unwrap();
    let (_, steer) = bridge.steer("also this").unwrap();
    let withdraw = withdrawal(&mut bridge);
    let next = bridge
        .inbound(success(&withdraw, json!({"cancelled":true})))
        .unwrap();
    assert!(ends(&next, 5).is_none(), "a message still waits");
    assert_eq!(
        next.replies[0]["request"],
        json!({"subtype":"cancel_async_message","message_uuid":steer})
    );
    let ended = bridge
        .inbound(success(&next.replies[0], json!({"cancelled":true})))
        .unwrap();
    assert!(
        ended.replies.is_empty(),
        "no interrupt to an idle Claude Code"
    );
    assert_eq!(
        ends(&ended, 5).unwrap()["result"]["stopReason"],
        "cancelled"
    );
}

#[test]
fn a_stop_does_not_wait_on_an_unanswered_withdrawal_while_a_run_goes_on() {
    let mut bridge = initialized();
    finish_prompted_turn(&mut bridge);
    prompt_with_uuid(&mut bridge, 5);
    bridge.inbound(run_start()).unwrap();
    bridge.inbound(prompt_lifecycle("queued")).unwrap();
    withdrawal(&mut bridge);
    let going = bridge.inbound(text("Still going")).unwrap();
    let interrupt = &going.replies[0];
    assert_eq!(interrupt["request"]["subtype"], "interrupt");
    bridge.inbound(success(interrupt, json!({}))).unwrap();
    assert!(
        bridge.inbound(text("More")).unwrap().replies.is_empty(),
        "one interrupt is enough"
    );
    bridge
        .inbound(result_frame("error_during_execution"))
        .unwrap();
    let ended = bridge.inbound(prompt_lifecycle("cancelled")).unwrap();
    assert_eq!(
        ends(&ended, 5).unwrap()["result"]["stopReason"],
        "cancelled"
    );
}

#[test]
fn a_withdrawal_claude_code_refuses_falls_back_to_the_interrupt() {
    let mut bridge = initialized();
    finish_prompted_turn(&mut bridge);
    prompt_with_uuid(&mut bridge, 5);
    bridge.inbound(prompt_lifecycle("queued")).unwrap();
    let withdraw = withdrawal(&mut bridge);
    let refused = bridge
        .inbound(
            json!({"type":"control_response","response":{"subtype":"error",
            "request_id":withdraw["request_id"],"error":"not queued"}}),
        )
        .unwrap();
    assert!(ends(&refused, 5).is_none());
    assert_eq!(refused.replies[0]["request"]["subtype"], "interrupt");
    let ended = bridge.inbound(prompt_lifecycle("cancelled")).unwrap();
    assert_eq!(
        ends(&ended, 5).unwrap()["result"]["stopReason"],
        "cancelled"
    );
}

#[test]
fn a_stop_on_a_build_without_lifecycle_reports_interrupts_at_once() {
    let mut bridge = initialized();
    prompt_with_uuid(&mut bridge, 5);
    bridge.inbound(run_start()).unwrap();
    let interrupt = cancel(&mut bridge);
    assert_eq!(interrupt["request"]["subtype"], "interrupt");
    bridge.inbound(success(&interrupt, json!({}))).unwrap();
    let end = bridge
        .inbound(result_frame("error_during_execution"))
        .unwrap();
    assert_eq!(ends(&end, 5).unwrap()["result"]["stopReason"], "cancelled");
}

fn interrupts(inbound: &Inbound) -> Vec<Value> {
    inbound
        .replies
        .iter()
        .filter(|reply| reply["request"]["subtype"] == "interrupt")
        .cloned()
        .collect()
}

#[test]
fn a_stop_left_unanswered_holds_up_neither_capacity_nor_the_next_stop() {
    let mut bridge = initialized();
    finish_prompted_turn(&mut bridge);
    prompt_with_uuid(&mut bridge, 5);
    bridge.inbound(run_start()).unwrap();
    bridge.inbound(prompt_lifecycle("queued")).unwrap();
    let unanswered = withdrawal(&mut bridge);
    let interrupt = bridge.inbound(text("Going")).unwrap().replies[0].clone();
    bridge.inbound(success(&interrupt, json!({}))).unwrap();
    bridge
        .inbound(result_frame("error_during_execution"))
        .unwrap();
    let ended = bridge.inbound(prompt_lifecycle("cancelled")).unwrap();
    assert!(ends(&ended, 5).is_some());
    assert!(
        !bridge
            .pending
            .values()
            .any(|pending| matches!(pending, Pending::Withdraw(_))),
        "the ended stop's withdrawal holds no capacity"
    );

    prompt_with_uuid(&mut bridge, 6);
    bridge.inbound(prompt_lifecycle("queued")).unwrap();
    let (_, steer) = bridge.steer("also this").unwrap();
    let withdraw = withdrawal(&mut bridge);
    let next = bridge
        .inbound(success(&withdraw, json!({"cancelled":true})))
        .unwrap();
    assert_eq!(
        next.replies[0]["request"],
        json!({"subtype":"cancel_async_message","message_uuid":steer})
    );
    let late = bridge
        .inbound(success(&unanswered, json!({"cancelled":true})))
        .unwrap();
    assert!(late.acp.is_empty() && late.replies.is_empty());
    let ended = bridge
        .inbound(success(&next.replies[0], json!({"cancelled":true})))
        .unwrap();
    assert_eq!(
        ends(&ended, 6).unwrap()["result"]["stopReason"],
        "cancelled"
    );
}

#[test]
fn a_stop_with_nothing_running_moves_on_without_the_withdrawals_reply() {
    let mut bridge = initialized();
    finish_prompted_turn(&mut bridge);
    prompt_with_uuid(&mut bridge, 5);
    bridge.inbound(prompt_lifecycle("queued")).unwrap();
    let (_, steer) = bridge.steer("also this").unwrap();
    withdrawal(&mut bridge);
    let next = bridge.inbound(prompt_lifecycle("cancelled")).unwrap();
    assert!(ends(&next, 5).is_none(), "a message still waits");
    assert_eq!(
        next.replies[0]["request"],
        json!({"subtype":"cancel_async_message","message_uuid":steer})
    );
    let ended = bridge
        .inbound(json!({"type":"command_lifecycle","command_uuid":steer,
            "state":"cancelled","uuid":"e2","session_id":SESSION}))
        .unwrap();
    assert!(
        ended.replies.is_empty(),
        "no interrupt to an idle Claude Code"
    );
    assert_eq!(
        ends(&ended, 5).unwrap()["result"]["stopReason"],
        "cancelled"
    );
}

#[test]
fn a_prompt_taken_up_while_stopping_gets_one_interrupt() {
    let mut bridge = initialized();
    finish_prompted_turn(&mut bridge);
    prompt_with_uuid(&mut bridge, 5);
    bridge.inbound(run_start()).unwrap();
    bridge.inbound(prompt_lifecycle("queued")).unwrap();
    let withdraw = withdrawal(&mut bridge);
    let mut sent = interrupts(&bridge.inbound(prompt_lifecycle("started")).unwrap());
    sent.extend(interrupts(
        &bridge
            .inbound(success(&withdraw, json!({"cancelled":false})))
            .unwrap(),
    ));
    sent.extend(interrupts(&bridge.inbound(text("Partly")).unwrap()));
    assert_eq!(sent.len(), 1, "one interrupt ends the run");
    bridge.inbound(success(&sent[0], json!({}))).unwrap();
    let end = bridge
        .inbound(result_frame("error_during_execution"))
        .unwrap();
    assert_eq!(ends(&end, 5).unwrap()["result"]["stopReason"], "cancelled");
}
