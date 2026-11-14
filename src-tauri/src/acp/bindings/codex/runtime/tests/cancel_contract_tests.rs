use super::*;
use sage_acp_protocol::contract::SageServerRequestId;

fn pending(runtime: &mut CodexRuntime, method: &str, request_id: &str) -> SageServerRequestId {
    ready(runtime);
    running_turn(runtime);
    runtime
        .ingest(
            &frame(json!({
                "id":request_id,"method":method,"params":{
                    "threadId":"thread-1","turnId":"turn-1","itemId":"tool-cancel"
                }
            })),
            &Mapper,
        )
        .expect("permission request");
    SageServerRequestId::parse(format!("string:{request_id}")).expect("durable request")
}

#[test]
fn cancel_uses_exact_native_approval_equivalent_then_turn_interrupt() {
    let mut runtime = fixture_runtime();
    let request = pending(
        &mut runtime,
        "item/commandExecution/requestApproval",
        "approval-cancel",
    );
    let response = runtime
        .cancel_permission(&request)
        .expect("native cancellation response");
    let body: Value = serde_json::from_slice(response.frame().payload()).expect("json");
    assert_eq!(
        body,
        json!({"id":"approval-cancel","result":{"decision":"decline"}})
    );
    assert!(body.get("jsonrpc").is_none(), "Codex is not ACP wire");
    let token = response.into_token();
    runtime
        .commit_mandatory_egress(&token)
        .expect("native cancellation committed");
    runtime
        .resolve_egress(token.clone(), EgressResolution::Queued)
        .expect("queued");
    runtime
        .resolve_egress(token, EgressResolution::Written)
        .expect("written");

    let duplicate = runtime
        .ingest(
            &frame(json!({
                "id":"approval-cancel","method":"item/commandExecution/requestApproval",
                "params":{"threadId":"thread-1","turnId":"turn-1","itemId":"tool-cancel"}
            })),
            &Mapper,
        )
        .expect("duplicate");
    assert!(duplicate.egress().is_empty());
    let interrupt = runtime.command(AdapterCommand::Cancel).expect("interrupt");
    let interrupt_body: Value =
        serde_json::from_slice(interrupt.frame().payload()).expect("interrupt json");
    assert_eq!(interrupt_body["method"], "turn/interrupt");
}

#[test]
fn granular_permission_cancel_returns_the_native_empty_permission_set() {
    let mut runtime = fixture_runtime();
    let request = pending(
        &mut runtime,
        "item/permissions/requestApproval",
        "permissions-cancel",
    );
    let response = runtime
        .cancel_permission(&request)
        .expect("native response");
    let body: Value = serde_json::from_slice(response.frame().payload()).expect("json");
    assert_eq!(
        body,
        json!({"id":"permissions-cancel","result":{"permissions":{}}})
    );
    assert!(body.get("jsonrpc").is_none());
}

#[test]
fn a_cancel_held_for_an_unnamed_turn_goes_out_with_the_frame_that_names_it() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    let mut prompt = Prompt::new("sessionAction", "/compact").expect("prompt");
    prompt.action = Some(serde_json::from_value(json!({"kind":"compact"})).expect("action"));
    let compact = runtime
        .command(AdapterCommand::Prompt {
            prompt,
            client_message_id: RequiredText::new("client", "client-compact").expect("client"),
            access_mode: sage_acp_protocol::access_mode::SessionAccessMode::ReadOnly,
        })
        .expect("compaction request");
    let id = request_id(&compact);
    runtime
        .ingest(&frame(json!({"id":id,"result":{}})), &Mapper)
        .expect("empty answer");
    assert!(runtime.provisional_turn_pending());
    assert!(runtime.defer_cancel());
    let started = runtime
        .ingest(
            &frame(
                json!({"method":"turn/started","params":{"threadId":"thread-1",
                "turn":{"id":"turn-compact","rootTurnId":"turn-compact","status":"inProgress"}}}),
            ),
            &Mapper,
        )
        .expect("turn started");
    let [interrupt] = started.egress() else {
        panic!("the held cancel goes out once");
    };
    assert!(interrupt.token().is_cancel());
    let body: Value = serde_json::from_slice(interrupt.frame().payload()).expect("json");
    assert_eq!(
        (&body["method"], &body["params"]["turnId"]),
        (&json!("turn/interrupt"), &json!("turn-compact"))
    );
    assert!(!runtime.provisional_turn_pending());
}
