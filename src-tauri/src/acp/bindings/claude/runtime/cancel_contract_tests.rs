use super::*;

#[test]
fn cancellation_response_preserves_original_acp_request_id_and_is_not_replayed() {
    let mut runtime = ClaudeRuntime::fixture();
    ready(&mut runtime);
    let (_prompt_id, prompt_token) = prompt(&mut runtime);
    runtime
        .resolve_egress(prompt_token, EgressResolution::Queued)
        .expect("prompt queued");
    runtime
        .ingest(
            &frame(json!({"method":"session/update","params":{
                "sessionId":"claude-session-1","update":{
                    "sessionUpdate":"tool_call","toolCallId":"tool-cancel",
                    "kind":"execute","status":"in_progress"
                }
            }})),
            &Mapper,
        )
        .expect("tool");
    let wire = json!({
        "jsonrpc":"2.0","id":41,"method":"session/request_permission","params":{
            "sessionId":"claude-session-1","toolCall":{"toolCallId":"tool-cancel"},
            "options":[{"optionId":"allow","name":"Allow"}]
        }
    });
    let batch = runtime
        .ingest(&frame(wire.clone()), &Mapper)
        .expect("permission");
    let SharedEvent::PermissionRequest {
        external_request_id,
        ..
    } = &batch.events()[0]
    else {
        panic!("permission event")
    };
    let request = SageServerRequestId::parse(external_request_id.as_str()).expect("request");
    let response = runtime
        .cancel_permission(&request)
        .expect("cancel response");
    let body: Value = serde_json::from_slice(response.frame().payload()).expect("json");
    assert_eq!(
        body,
        json!({"jsonrpc":"2.0","id":41,"result":{"outcome":{"outcome":"cancelled"}}})
    );
    let token = response.into_token();
    runtime.commit_mandatory_egress(&token).expect("committed");
    runtime
        .resolve_egress(token.clone(), EgressResolution::Queued)
        .expect("queued");
    runtime
        .resolve_egress(token, EgressResolution::Written)
        .expect("written");
    let duplicate = runtime.ingest(&frame(wire), &Mapper).expect("duplicate");
    assert!(duplicate.egress().is_empty());
}
