use crate::acp::{
    bindings::adapter::{AdapterEgress, EgressToken},
    runtime::{
        later_permission::{LaterPermissionError, resolve_later_permission},
        permissions::PermissionDecision,
    },
    transport::OutboundFrame,
};

fn request() -> crate::acp::contract::SageServerRequestId {
    crate::acp::contract::SageServerRequestId::parse("permission-request-1").unwrap()
}

fn reply() -> AdapterEgress {
    AdapterEgress::new(
        OutboundFrame::new(b"{\"jsonrpc\":\"2.0\"}".to_vec()).unwrap(),
        EgressToken::protocol_notification(),
    )
}

#[test]
fn allow_once_replaces_the_adapter_token_without_a_mandatory_deny() {
    let result = resolve_later_permission(&request(), PermissionDecision::AllowOnce, Some(reply()))
        .expect("pending request")
        .expect("allow has wire reply");
    assert!(matches!(result.token().deny_request(), None));
}

#[test]
fn allow_once_uses_the_permission_allow_token() {
    let result = resolve_later_permission(&request(), PermissionDecision::AllowOnce, Some(reply()))
        .expect("pending request")
        .expect("allow has wire reply");
    assert!(!result.token().is_mandatory());
}

#[test]
fn request_id_is_the_permission_allow_token_identity() {
    let request = crate::acp::contract::SageServerRequestId::parse("approval-1").unwrap();
    let egress = resolve_later_permission(&request, PermissionDecision::AllowOnce, Some(reply()))
        .unwrap()
        .unwrap();
    assert_eq!(egress.token().allow_request(), Some("approval-1"));
}

#[test]
fn request_id_is_the_permission_deny_token_identity() {
    let request = crate::acp::contract::SageServerRequestId::parse("pi-request-1").unwrap();
    let egress = resolve_later_permission(&request, PermissionDecision::Deny, Some(reply()))
        .unwrap()
        .unwrap();
    assert_eq!(egress.token().deny_request(), Some(("pi-request-1", false)));
}

#[test]
fn missing_pending_request_is_a_typed_later_decision_miss() {
    assert!(matches!(
        resolve_later_permission(&request(), PermissionDecision::Deny, None),
        Err(LaterPermissionError::PendingRequestMiss)
    ));
}
