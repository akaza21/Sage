use super::*;

fn declined_before_turn_end(events: &[SharedEvent]) -> (Option<usize>, Option<usize>) {
    let declined = events.iter().position(|event| {
        matches!(
            event,
            SharedEvent::Item {
                tool_state: Some(ToolState::Declined),
                ..
            }
        )
    });
    let turn = events
        .iter()
        .position(|event| matches!(event, SharedEvent::Turn { .. }));
    (declined, turn)
}

#[test]
fn a_committed_decline_reaches_the_durable_card_before_the_turn_ends() {
    let mut runtime = ClaudeRuntime::fixture();
    ready(&mut runtime);
    let (prompt_id, token) = prompt(&mut runtime);
    runtime
        .resolve_egress(token, EgressResolution::Queued)
        .expect("prompt queued");
    runtime
        .ingest(
            &frame(json!({"method":"session/update","params":{
                "sessionId":"claude-session-1","update":{
                    "sessionUpdate":"tool_call","toolCallId":"tool-deny",
                    "kind":"execute","status":"in_progress"
                }
            }})),
            &Mapper,
        )
        .expect("tool start");
    let batch = runtime
        .ingest(
            &frame(json!({
                "id":"permission-deny","method":"session/request_permission","params":{
                    "sessionId":"claude-session-1","toolCall":{"toolCallId":"tool-deny"},
                    "options":[{"optionId":"allow","name":"Allow"}]
                }
            })),
            &Mapper,
        )
        .expect("permission");
    let SharedEvent::PermissionRequest {
        external_request_id,
        ..
    } = &batch.events()[0]
    else {
        panic!("permission request")
    };
    let request = SageServerRequestId::parse(external_request_id.as_str()).expect("request");
    let deny = runtime
        .command(AdapterCommand::ResolvePermission {
            request_id: request,
            decision: sage_acp_protocol::permissions::PermissionDecision::Deny,
        })
        .expect("deny");
    assert!(deny.token().is_mandatory());
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
    let terminal = runtime
        .ingest(
            &frame(json!({
                "jsonrpc":"2.0","id":prompt_id,"result":{"stopReason":"end_turn"}
            })),
            &Mapper,
        )
        .expect("prompt result");
    let (declined, turn) = declined_before_turn_end(terminal.events());
    let declined = declined.expect("the declined tool card is published");
    let turn = turn.expect("the turn terminal is published");
    assert!(
        declined < turn,
        "the declined card must land before the turn ends"
    );
}

fn start_tool(runtime: &mut ClaudeRuntime, tool: &str) {
    runtime
        .ingest(
            &frame(json!({"method":"session/update","params":{
                "sessionId":"claude-session-1","update":{
                    "sessionUpdate":"tool_call","toolCallId":tool,
                    "kind":"execute","status":"in_progress"
                }
            }})),
            &Mapper,
        )
        .expect("tool start");
}

fn ask_permission(runtime: &mut ClaudeRuntime, request: &str, tool: &str) -> AdapterBatch {
    runtime
        .ingest(
            &frame(json!({
                "id":request,"method":"session/request_permission","params":{
                    "sessionId":"claude-session-1","toolCall":{"toolCallId":tool},
                    "options":[{"optionId":"allow","name":"Allow"}]
                }
            })),
            &Mapper,
        )
        .expect("permission")
}

fn deny_outstanding(runtime: &mut ClaudeRuntime, batch: &AdapterBatch) {
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
    let request = SageServerRequestId::parse(request.as_str()).expect("request");
    let deny = runtime
        .command(AdapterCommand::ResolvePermission {
            request_id: request,
            decision: sage_acp_protocol::permissions::PermissionDecision::Deny,
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
}

#[test]
fn a_new_permission_frame_after_a_decline_still_carries_the_declined_card_first() {
    let mut runtime = ClaudeRuntime::fixture();
    ready(&mut runtime);
    let (_prompt_id, token) = prompt(&mut runtime);
    runtime
        .resolve_egress(token, EgressResolution::Queued)
        .expect("prompt queued");
    start_tool(&mut runtime, "tool-first");
    start_tool(&mut runtime, "tool-second");
    let outstanding = ask_permission(&mut runtime, "permission-first", "tool-first");
    deny_outstanding(&mut runtime, &outstanding);
    let next = ask_permission(&mut runtime, "permission-second", "tool-second");
    let events = next.events();
    let declined = events
        .iter()
        .position(|event| {
            matches!(
                event,
                SharedEvent::Item {
                    tool_state: Some(ToolState::Declined),
                    ..
                }
            )
        })
        .expect("the declined card rides the next permission frame");
    let requested = events
        .iter()
        .position(|event| matches!(event, SharedEvent::PermissionRequest { .. }))
        .expect("the new permission request is published");
    assert!(
        declined < requested,
        "the declined card precedes the new permission request"
    );
}
