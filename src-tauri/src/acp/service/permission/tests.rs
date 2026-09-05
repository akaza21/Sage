mod support;

use std::sync::{Arc, Mutex};

use crate::acp::{
    bindings::adapter::AdapterCommand,
    contract::{
        RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId, SageServerRequestId,
        SageTurnId,
    },
    runtime::permissions::PermissionDecision,
};

use super::PermissionDecisionIntent;
use support::{permission_snapshot, service_with_pending_requests};

#[test]
fn resolve_permission_decision_reaches_live_runtime_port() {
    let commands = Arc::new(Mutex::new(Vec::new()));
    let (service, owner) = service_with_pending_requests(Arc::clone(&commands));
    for (request_id, decision) in [
        ("request-deny", PermissionDecision::Deny),
        ("request-allow", PermissionDecision::AllowOnce),
        ("request-ask", PermissionDecision::Ask),
    ] {
        let response = service
            .resolve_permission_decision(PermissionDecisionIntent {
                project_id: owner.project_id.clone(),
                chat_id: owner.chat_id.clone(),
                session_id: owner.session_id.clone(),
                turn_id: SageTurnId::parse("turn-permission").unwrap(),
                runtime_epoch: owner.runtime_epoch,
                request_id: SageServerRequestId::parse(request_id).unwrap(),
                decision,
            })
            .unwrap();
        let response = serde_json::to_value(response).unwrap();
        assert_eq!(response["projectId"], owner.project_id.as_str());
        assert_eq!(response["chatId"], owner.chat_id.as_str());
        assert_eq!(response["sessionId"], owner.session_id.as_str());
        assert_eq!(response["turnId"], "turn-permission");
        assert_eq!(response["runtimeEpoch"], "1");
        assert_eq!(response["requestId"], request_id);
    }
    let decisions = commands
        .lock()
        .unwrap()
        .drain(..)
        .map(|command| match command {
            AdapterCommand::ResolvePermission {
                request_id,
                decision,
            } => (request_id, decision),
            _ => panic!("permission routing must use AdapterCommand::ResolvePermission"),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        decisions
            .iter()
            .map(|(_, decision)| *decision)
            .collect::<Vec<_>>(),
        vec![
            PermissionDecision::Deny,
            PermissionDecision::AllowOnce,
            PermissionDecision::Ask,
        ]
    );
    service.shutdown_blocking().unwrap();
}

#[derive(Clone)]
struct DecisionIdentity {
    project_id: SageProjectId,
    chat_id: SageChatId,
    session_id: SageAgentSessionId,
    turn_id: SageTurnId,
    runtime_epoch: RuntimeEpoch,
    request_id: SageServerRequestId,
}

#[test]
fn every_forged_decision_identity_field_has_zero_durable_or_runtime_side_effects() {
    let commands = Arc::new(Mutex::new(Vec::new()));
    let (service, owner) = service_with_pending_requests(Arc::clone(&commands));
    let exact = DecisionIdentity {
        project_id: owner.project_id.clone(),
        chat_id: owner.chat_id.clone(),
        session_id: owner.session_id.clone(),
        turn_id: SageTurnId::parse("turn-permission").unwrap(),
        runtime_epoch: owner.runtime_epoch,
        request_id: SageServerRequestId::parse("request-deny").unwrap(),
    };
    for field in [
        "projectId",
        "chatId",
        "sessionId",
        "turnId",
        "runtimeEpoch",
        "requestId",
    ] {
        let mut forged = exact.clone();
        match field {
            "projectId" => forged.project_id = SageProjectId::parse("project-forged").unwrap(),
            "chatId" => forged.chat_id = SageChatId::parse("chat-forged").unwrap(),
            "sessionId" => forged.session_id = SageAgentSessionId::parse("session-forged").unwrap(),
            "turnId" => forged.turn_id = SageTurnId::parse("turn-forged").unwrap(),
            "runtimeEpoch" => forged.runtime_epoch = RuntimeEpoch::ZERO,
            "requestId" => {
                forged.request_id = SageServerRequestId::parse("request-forged").unwrap()
            }
            _ => unreachable!("fixed forged field table"),
        }
        let before = permission_snapshot(&service);

        assert!(
            service
                .resolve_permission_decision(PermissionDecisionIntent {
                    project_id: forged.project_id,
                    chat_id: forged.chat_id,
                    session_id: forged.session_id,
                    turn_id: forged.turn_id,
                    runtime_epoch: forged.runtime_epoch,
                    request_id: forged.request_id,
                    decision: PermissionDecision::Deny,
                })
                .is_err(),
            "{field} must fail closed"
        );

        assert_eq!(permission_snapshot(&service), before, "{field}");
        assert!(commands.lock().unwrap().is_empty(), "{field}");
    }
    service.shutdown_blocking().unwrap();
}
