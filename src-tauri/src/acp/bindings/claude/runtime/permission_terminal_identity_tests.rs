use super::*;

use crate::acp::intent::{NormalizedPayload, ToolCategory};

const TOOL_TITLE: &str = "Write hr6-declined.txt";

fn terminal_card(events: &[SharedEvent], state: ToolState) -> (String, ToolCategory, bool) {
    for event in events {
        if let SharedEvent::Item {
            tool_state: Some(seen),
            payload:
                NormalizedPayload::ToolCall {
                    label,
                    category,
                    input_summary,
                    ..
                },
            ..
        } = event
            && *seen == state
        {
            return (
                label.as_str().to_owned(),
                *category,
                input_summary.is_some(),
            );
        }
    }
    panic!("a terminal tool card is published");
}

fn edit_tool(runtime: &mut ClaudeRuntime, tool: &str) {
    runtime
        .ingest(
            &frame(json!({"method":"session/update","params":{
                "sessionId":"claude-session-1","update":{
                    "sessionUpdate":"tool_call","toolCallId":tool,
                    "kind":"edit","status":"in_progress","title":TOOL_TITLE,
                    "rawInput":{"command":"write hr6-declined.txt"}
                }
            }})),
            &Mapper,
        )
        .expect("tool start");
}

fn outstanding_permission(runtime: &mut ClaudeRuntime, tool: &str) -> SageServerRequestId {
    outstanding_permission_with_batch(runtime, tool).0
}

fn untyped_tool(runtime: &mut ClaudeRuntime, tool: &str) {
    runtime
        .ingest(
            &frame(json!({"method":"session/update","params":{
                "sessionId":"claude-session-1","update":{
                    "sessionUpdate":"tool_call","toolCallId":tool,
                    "kind":"other","status":"in_progress","title":TOOL_TITLE,
                    "rawInput":{"command":"write hr6-declined.txt"}
                }
            }})),
            &Mapper,
        )
        .expect("tool start");
}

fn outstanding_permission_with_batch(
    runtime: &mut ClaudeRuntime,
    tool: &str,
) -> (SageServerRequestId, AdapterBatch) {
    let batch = runtime
        .ingest(
            &frame(json!({
                "id":"permission-identity","method":"session/request_permission","params":{
                    "sessionId":"claude-session-1","toolCall":{"toolCallId":tool,
                        "kind":"edit","title":TOOL_TITLE,
                        "rawInput":{"command":"write hr6-declined.txt"}},
                    "options":[{"optionId":"allow","name":"Allow"}]
                }
            })),
            &Mapper,
        )
        .expect("permission");
    let request = batch
        .events()
        .iter()
        .find_map(|event| match event {
            SharedEvent::PermissionRequest {
                external_request_id,
                ..
            } => Some(external_request_id.as_str().to_owned()),
            _ => None,
        })
        .expect("permission request");
    (
        SageServerRequestId::parse(request.as_str()).expect("request"),
        batch,
    )
}

fn end_turn(runtime: &mut ClaudeRuntime, prompt_id: u64) -> AdapterBatch {
    runtime
        .ingest(
            &frame(json!({
                "jsonrpc":"2.0","id":prompt_id,"result":{"stopReason":"end_turn"}
            })),
            &Mapper,
        )
        .expect("prompt result")
}

#[test]
fn a_declined_card_keeps_the_label_and_kind_the_tool_already_carried() {
    let mut runtime = ClaudeRuntime::fixture();
    ready(&mut runtime);
    let (prompt_id, token) = prompt(&mut runtime);
    runtime
        .resolve_egress(token, EgressResolution::Queued)
        .expect("prompt queued");
    edit_tool(&mut runtime, "tool-identity-deny");
    let request = outstanding_permission(&mut runtime, "tool-identity-deny");
    let deny = runtime
        .command(AdapterCommand::ResolvePermission {
            request_id: request,
            decision: crate::acp_protocol::permissions::PermissionDecision::Deny,
        })
        .expect("deny");
    let token = deny.into_token();
    runtime
        .commit_mandatory_egress(&token)
        .expect("decline commits");
    runtime
        .resolve_egress(token.clone(), EgressResolution::Queued)
        .expect("queued");
    runtime
        .resolve_egress(token, EgressResolution::Written)
        .expect("written");
    let terminal = end_turn(&mut runtime, prompt_id);
    let (label, category, kept_input) = terminal_card(terminal.events(), ToolState::Declined);
    assert_eq!(label, TOOL_TITLE, "the declined card keeps its label");
    assert_eq!(
        category,
        ToolCategory::Edit,
        "the declined card keeps its kind"
    );
    assert!(kept_input, "the declined card keeps its input summary");
}

#[test]
fn a_cancelled_card_keeps_the_label_and_kind_the_tool_already_carried() {
    let mut runtime = ClaudeRuntime::fixture();
    ready(&mut runtime);
    let (prompt_id, token) = prompt(&mut runtime);
    runtime
        .resolve_egress(token, EgressResolution::Queued)
        .expect("prompt queued");
    edit_tool(&mut runtime, "tool-identity-cancel");
    let request = outstanding_permission(&mut runtime, "tool-identity-cancel");
    let response = runtime
        .cancel_permission(&request)
        .expect("cancel response");
    let token = response.into_token();
    runtime.commit_mandatory_egress(&token).expect("committed");
    runtime
        .resolve_egress(token.clone(), EgressResolution::Queued)
        .expect("queued");
    runtime
        .resolve_egress(token, EgressResolution::Written)
        .expect("written");
    let terminal = end_turn(&mut runtime, prompt_id);
    let (label, category, kept_input) = terminal_card(terminal.events(), ToolState::Cancelled);
    assert_eq!(label, TOOL_TITLE, "the cancelled card keeps its label");
    assert_eq!(
        category,
        ToolCategory::Edit,
        "the cancelled card keeps its kind"
    );
    assert!(kept_input, "the cancelled card keeps its input summary");
}

#[test]
fn a_declined_card_keeps_the_kind_the_permission_surface_showed() {
    let mut runtime = ClaudeRuntime::fixture();
    ready(&mut runtime);
    let (prompt_id, token) = prompt(&mut runtime);
    runtime
        .resolve_egress(token, EgressResolution::Queued)
        .expect("prompt queued");
    untyped_tool(&mut runtime, "tool-kind-deny");
    let (request, waiting) = outstanding_permission_with_batch(&mut runtime, "tool-kind-deny");
    let (_, waiting_category, _) = terminal_card(waiting.events(), ToolState::WaitingForApproval);
    assert_eq!(
        waiting_category,
        ToolCategory::Edit,
        "the waiting card shows the permission kind"
    );
    let deny = runtime
        .command(AdapterCommand::ResolvePermission {
            request_id: request,
            decision: crate::acp_protocol::permissions::PermissionDecision::Deny,
        })
        .expect("deny");
    let token = deny.into_token();
    runtime
        .commit_mandatory_egress(&token)
        .expect("decline commits");
    runtime
        .resolve_egress(token.clone(), EgressResolution::Queued)
        .expect("queued");
    runtime
        .resolve_egress(token, EgressResolution::Written)
        .expect("written");
    let terminal = end_turn(&mut runtime, prompt_id);
    let (_, category, _) = terminal_card(terminal.events(), ToolState::Declined);
    assert_eq!(
        category,
        ToolCategory::Edit,
        "the declined card keeps the permission kind"
    );
}

#[test]
fn a_cancelled_card_keeps_the_kind_the_permission_surface_showed() {
    let mut runtime = ClaudeRuntime::fixture();
    ready(&mut runtime);
    let (prompt_id, token) = prompt(&mut runtime);
    runtime
        .resolve_egress(token, EgressResolution::Queued)
        .expect("prompt queued");
    untyped_tool(&mut runtime, "tool-kind-cancel");
    let (request, waiting) = outstanding_permission_with_batch(&mut runtime, "tool-kind-cancel");
    let (_, waiting_category, _) = terminal_card(waiting.events(), ToolState::WaitingForApproval);
    assert_eq!(
        waiting_category,
        ToolCategory::Edit,
        "the waiting card shows the permission kind"
    );
    let response = runtime
        .cancel_permission(&request)
        .expect("cancel response");
    let token = response.into_token();
    runtime.commit_mandatory_egress(&token).expect("committed");
    runtime
        .resolve_egress(token.clone(), EgressResolution::Queued)
        .expect("queued");
    runtime
        .resolve_egress(token, EgressResolution::Written)
        .expect("written");
    let terminal = end_turn(&mut runtime, prompt_id);
    let (_, category, _) = terminal_card(terminal.events(), ToolState::Cancelled);
    assert_eq!(
        category,
        ToolCategory::Edit,
        "the cancelled card keeps the permission kind"
    );
}
