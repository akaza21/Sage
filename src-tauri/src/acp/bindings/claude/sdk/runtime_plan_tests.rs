//! Plan approval and pending edit cards, from frames Claude Code 2.1.330
//! sends in plan mode.

use super::*;
use sage_acp_protocol::permissions::PermissionDecision;

const PLAN: &str = "# Plan: greet.js prints 'hello'\n\n- Edit `greet.js` line 1: change \
`console.log('hi')` to `console.log('hello')`.\n- Verify by running `node greet.js`.\n";

fn prompt(runtime: &mut ClaudeSdkRuntime) {
    send(
        runtime,
        AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "Plan the change").unwrap(),
            client_message_id: RequiredText::new("client", "client-1").unwrap(),
            access_mode: sage_acp_protocol::access_mode::SessionAccessMode::Ask,
        },
    );
}

/// Feeds Claude's tool use and its approval request; returns the card's
/// request id and summary.
fn card(runtime: &mut ClaudeSdkRuntime, name: &str, input: Value) -> (String, String) {
    feed(
        runtime,
        json!({"type":"assistant","parent_tool_use_id":null,"message":{"id":"m","content":[
            {"type":"tool_use","id":"t1","name":name,"input":input}]}}),
    );
    let events = feed(
        runtime,
        json!({"type":"control_request","request_id":"cli-1","request":{
            "subtype":"can_use_tool","tool_name":name,"display_name":name,"input":input,
            "tool_use_id":"t1","requires_user_interaction":true}}),
    );
    let request = events.iter().find_map(|event| match event {
        AdapterEvent::PermissionRequest {
            external_request_id,
            ..
        } => Some(external_request_id.as_str().to_owned()),
        _ => None,
    });
    let summary = events.iter().find_map(|event| match event {
        AdapterEvent::Item {
            payload: NormalizedPayload::PermissionRequest { summary, .. },
            ..
        } => Some(summary.as_str().to_owned()),
        _ => None,
    });
    (request.expect("card"), summary.expect("summary"))
}

fn plan_input() -> Value {
    json!({"plan": PLAN, "planFilePath": "/home/u/.claude/plans/plan.md"})
}

#[test]
fn a_plan_card_shows_the_plan_it_asks_to_approve() {
    let mut runtime = ready();
    prompt(&mut runtime);
    let (_, summary) = card(&mut runtime, "ExitPlanMode", plan_input());
    assert_eq!(summary, format!("Ready to code?\n\n{}", PLAN.trim()));
}

#[test]
fn declining_a_plan_lets_the_turn_end_as_claude_ends_it() {
    let mut runtime = ready();
    prompt(&mut runtime);
    let (request, _) = card(&mut runtime, "ExitPlanMode", plan_input());
    let egress = runtime
        .command(AdapterCommand::ResolvePermission {
            request_id: SageServerRequestId::parse(&request).unwrap(),
            decision: PermissionDecision::Deny,
        })
        .unwrap();
    let sdk: Value = serde_json::from_slice(egress.frame().payload()).unwrap();
    // An interrupt makes Claude Code end with `error_during_execution`
    // ("[ede_diagnostic] … stop_reason=tool_use"), a failed turn.
    assert_eq!(
        sdk["response"]["response"],
        json!({"behavior":"deny","message":
            "The user chose to keep planning. Stop and wait for the user to say what to change."})
    );
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Written)
        .unwrap();
    feed(
        &mut runtime,
        json!({"type":"user","message":{"role":"user","content":[{"type":"tool_result",
            "content":"The user chose to keep planning. Stop and wait for the user to say what to change.",
            "is_error":true,"tool_use_id":"t1"}]},"parent_tool_use_id":null}),
    );
    feed(
        &mut runtime,
        json!({"type":"assistant","parent_tool_use_id":null,"message":{"id":"m2","content":[
            {"type":"text","text":"I've stopped. Tell me what to change."}]}}),
    );
    let events = feed(
        &mut runtime,
        json!({"type":"result","subtype":"success","is_error":false,"stop_reason":"end_turn",
            "result":"I've stopped. Tell me what to change.","num_turns":3}),
    );
    assert!(
        events.iter().any(|event| matches!(
            event,
            AdapterEvent::Turn {
                state: TurnState::Completed,
                ..
            }
        )),
        "{events:?}"
    );
}

#[test]
fn an_edit_card_asks_before_the_edit_rather_than_reporting_it() {
    let mut runtime = ready();
    prompt(&mut runtime);
    let (_, summary) = card(
        &mut runtime,
        "Edit",
        json!({"file_path":"/w/src/greet.js","old_string":"console.log('hi')",
            "new_string":"console.log('hello')","replace_all":false}),
    );
    assert_eq!(summary, "Edit src/greet.js?");
}
