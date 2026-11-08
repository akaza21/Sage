//! `$/cancel_request` for a permission the adapter stopped waiting for.

use super::*;

/// Opens permission request 7 in a running Turn and returns its Sage id.
fn open_permission(runtime: &mut ClaudeRuntime) -> (u64, SageServerRequestId) {
    let (prompt_id, token) = prompt(runtime);
    runtime
        .resolve_egress(token, EgressResolution::Written)
        .unwrap();
    runtime
        .ingest(
            &frame(json!({"jsonrpc":"2.0","method":"session/update","params":{
                "sessionId":"claude-session-1","update":{
                    "sessionUpdate":"tool_call","toolCallId":"tool-1",
                    "kind":"edit","status":"in_progress"
                }
            }})),
            &Mapper,
        )
        .unwrap();
    let batch = runtime
        .ingest(
            &frame(json!({
                "jsonrpc":"2.0","id":7,"method":"session/request_permission","params":{
                    "sessionId":"claude-session-1",
                    "toolCall":{"toolCallId":"tool-1","title":"Write file","kind":"edit",
                        "status":"pending","rawInput":{}},
                    "options":[
                        {"optionId":"allow","name":"Allow","kind":"allow_once"},
                        {"optionId":"reject","name":"Reject","kind":"reject_once"}
                    ]
                }
            })),
            &Mapper,
        )
        .unwrap();
    let Some(SharedEvent::PermissionRequest {
        external_request_id,
        ..
    }) = batch.events().last()
    else {
        panic!("permission card")
    };
    (
        prompt_id,
        SageServerRequestId::parse(external_request_id.as_str()).unwrap(),
    )
}

#[test]
fn a_permission_the_adapter_stops_waiting_for_is_settled_and_answered_cancelled() {
    let mut runtime = ClaudeRuntime::fixture();
    ready_0101(&mut runtime);
    let (prompt_id, request) = open_permission(&mut runtime);
    let cancel = runtime.command(AdapterCommand::Cancel).unwrap();
    runtime
        .resolve_egress(cancel.into_token(), EgressResolution::Written)
        .unwrap();

    let batch = runtime
        .ingest(&frame(derived("cancelPermissionRequest")), &Mapper)
        .unwrap();
    let [
        SharedEvent::PermissionResolved {
            external_request_id,
            state,
            ..
        },
    ] = batch.events()
    else {
        panic!("the open card is settled: {:?}", batch.events().len())
    };
    assert_eq!(external_request_id.as_str(), request.as_str());
    assert_eq!(*state, ServerRequestState::Cancelled);
    // SDK 1.x keeps waiting for this answer; SDK 2.x drops it quietly.
    let [reply] = batch.egress() else {
        panic!("one cancelled answer")
    };
    assert_eq!(
        payload(reply),
        json!({"jsonrpc":"2.0","id":7,"result":{"outcome":{"outcome":"cancelled"}}})
    );
    let token = reply.token().clone();
    runtime.commit_mandatory_egress(&token).unwrap();
    runtime
        .resolve_egress(token.clone(), EgressResolution::Queued)
        .unwrap();
    runtime
        .resolve_egress(token, EgressResolution::Written)
        .unwrap();
    // The card can no longer be answered by the user.
    assert!(runtime.cancel_permission(&request).is_err());
    // A repeated notice is late, and the cancelled prompt ends the Turn.
    let again = runtime
        .ingest(&frame(derived("cancelPermissionRequest")), &Mapper)
        .unwrap();
    assert!(matches!(again.events(), [SharedEvent::IgnoredLate]));
    let end = runtime
        .ingest(
            &frame(json!({"jsonrpc":"2.0","id":prompt_id,"result":{"stopReason":"cancelled"}})),
            &Mapper,
        )
        .unwrap();
    assert!(end.events().iter().any(|event| matches!(
        event,
        SharedEvent::Turn {
            state: TurnState::Cancelled,
            ..
        }
    )));
}

#[test]
fn a_permission_sage_already_cancelled_is_not_settled_twice() {
    let mut runtime = ClaudeRuntime::fixture();
    ready_0101(&mut runtime);
    let (_, request) = open_permission(&mut runtime);
    let reply = runtime.cancel_permission(&request).unwrap();
    let token = reply.into_token();
    runtime.commit_mandatory_egress(&token).unwrap();
    runtime
        .resolve_egress(token.clone(), EgressResolution::Queued)
        .unwrap();
    runtime
        .resolve_egress(token, EgressResolution::Written)
        .unwrap();
    let batch = runtime
        .ingest(&frame(derived("cancelPermissionRequest")), &Mapper)
        .unwrap();
    assert!(matches!(batch.events(), [SharedEvent::IgnoredLate]));
    assert!(batch.egress().is_empty());
}

#[test]
fn a_cancel_request_for_no_permission_stays_an_unsupported_notice() {
    let mut runtime = ClaudeRuntime::fixture();
    ready_0101(&mut runtime);
    let batch = runtime
        .ingest(
            &frame(json!({"jsonrpc":"2.0","method":"$/cancel_request","params":{"requestId":99}})),
            &Mapper,
        )
        .unwrap();
    assert!(matches!(batch.events(), [SharedEvent::Unsupported(_)]));
}
