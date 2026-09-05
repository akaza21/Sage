use super::*;

#[test]
fn cancellation_uses_exact_acp_response_and_duplicate_request_is_not_replayed() {
    let mut runtime = fixture_runtime(73);
    initialize(&mut runtime, false);
    new_session(&mut runtime);
    running(&mut runtime);

    let permission = permission(73, "permission-cancelled");
    let batch = runtime.ingest(&permission, &Mapper).expect("permission");
    assert!(matches!(
        batch.events(),
        [AdapterEvent::PermissionRequest { .. }]
    ));

    let request_id = SageServerRequestId::parse("73").expect("request id");
    let cancellation = runtime
        .cancel_permission(&request_id)
        .expect("cancellation response");
    assert_eq!(
        request(&cancellation),
        json!({"jsonrpc":"2.0","id":73,"result":{"outcome":{"outcome":"cancelled"}}})
    );
    let token = cancellation.into_token();
    runtime
        .commit_mandatory_egress(&token)
        .expect("durable cancellation commit");
    runtime
        .resolve_egress(token.clone(), EgressResolution::Queued)
        .expect("queued");
    runtime
        .resolve_egress(token, EgressResolution::Written)
        .expect("written");

    let duplicate = runtime.ingest(&permission, &Mapper).expect("duplicate");
    assert!(duplicate.egress().is_empty());
    assert!(matches!(
        duplicate.events(),
        [AdapterEvent::IgnoredDuplicate]
    ));
}
