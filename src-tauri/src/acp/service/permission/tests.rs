pub(crate) mod support;

use std::sync::{Arc, Mutex};

use crate::{
    acp::{
        bindings::adapter::AdapterCommand,
        contract::{
            RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId, SageServerRequestId,
            SageTurnId,
        },
    },
    acp_protocol::permissions::PermissionDecision,
};

use super::PermissionDecisionIntent;
use support::{
    permission_snapshot, permission_snapshot_for, service_with_pending_requests,
    service_with_pending_requests_and_deny_id,
};

#[test]
fn an_unoffered_allow_leaves_durable_and_runtime_state_untouched() {
    let commands = Arc::new(Mutex::new(Vec::new()));
    let (service, owner) = service_with_pending_requests(Arc::clone(&commands));
    let before = permission_snapshot(&service);
    assert!(
        service
            .resolve_permission_decision(PermissionDecisionIntent {
                project_id: owner.project_id.clone(),
                chat_id: owner.chat_id.clone(),
                session_id: owner.session_id.clone(),
                turn_id: SageTurnId::parse("turn-permission").unwrap(),
                runtime_epoch: owner.runtime_epoch,
                request_id: SageServerRequestId::parse("request-deny").unwrap(),
                decision: PermissionDecision::AllowOnce,
            })
            .is_err()
    );
    assert_eq!(permission_snapshot(&service), before);
    assert!(commands.lock().unwrap().is_empty());
    service.shutdown_blocking().unwrap();
}

#[test]
fn an_unoffered_always_allow_leaves_durable_and_runtime_state_untouched() {
    let commands = Arc::new(Mutex::new(Vec::new()));
    let (service, owner) = service_with_pending_requests(Arc::clone(&commands));
    let before = permission_snapshot(&service);
    assert!(
        service
            .resolve_permission_decision(PermissionDecisionIntent {
                project_id: owner.project_id.clone(),
                chat_id: owner.chat_id.clone(),
                session_id: owner.session_id.clone(),
                turn_id: SageTurnId::parse("turn-permission").unwrap(),
                runtime_epoch: owner.runtime_epoch,
                request_id: SageServerRequestId::parse("request-allow").unwrap(),
                decision: PermissionDecision::AllowAlways,
            })
            .is_err()
    );
    assert_eq!(permission_snapshot(&service), before);
    assert!(commands.lock().unwrap().is_empty());
    service.shutdown_blocking().unwrap();
}

#[test]
fn raw_external_id_is_rejected_before_runtime_dispatch_and_scoped_id_remains_usable() {
    let commands = Arc::new(Mutex::new(Vec::new()));
    let (service, owner) =
        service_with_pending_requests_and_deny_id(Arc::clone(&commands), "permission:scoped-deny");
    let scoped = SageServerRequestId::parse("permission:scoped-deny").unwrap();
    let before = permission_snapshot_for(&service, scoped.as_str());
    let decision = |request_id| PermissionDecisionIntent {
        project_id: owner.project_id.clone(),
        chat_id: owner.chat_id.clone(),
        session_id: owner.session_id.clone(),
        turn_id: SageTurnId::parse("turn-permission").unwrap(),
        runtime_epoch: owner.runtime_epoch,
        request_id,
        decision: PermissionDecision::Deny,
    };

    assert!(
        service
            .resolve_permission_decision(decision(
                SageServerRequestId::parse("request-deny").unwrap()
            ))
            .is_err()
    );
    assert_eq!(permission_snapshot_for(&service, scoped.as_str()), before);
    assert!(commands.lock().unwrap().is_empty());

    service
        .resolve_permission_decision(decision(scoped.clone()))
        .unwrap();
    assert_eq!(commands.lock().unwrap().len(), 1);
    service.shutdown_blocking().unwrap();
}

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
            .map(|(_, decision)| decision.clone())
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

#[test]
fn only_matching_answers_resolve_a_question_and_approvals_never_take_answers() {
    use crate::acp::{
        contract::{BoundedVec, SafeText},
        intent::{NormalizedPayload, PermissionOptionKind, ToolCategory},
    };
    use serde_json::json;

    let request =
        |offered: Vec<PermissionOptionKind>, user_input| NormalizedPayload::PermissionRequest {
            category: ToolCategory::Other,
            summary: SafeText::new("summary", "request").unwrap(),
            offered_option_kinds: Some(BoundedVec::new("offered", offered).unwrap()),
            reason: None,
            user_input,
        };
    let approval = request(
        vec![
            PermissionOptionKind::AllowOnce,
            PermissionOptionKind::RejectOnce,
        ],
        None,
    );
    let question = request(
        vec![PermissionOptionKind::RejectOnce],
        Some(Box::new(
            serde_json::from_value(json!({"questions": [{
                "id": "q", "header": "", "question": "?",
                "options": [{"label": "A", "description": ""}]
            }]}))
            .unwrap(),
        )),
    );
    let answer = |value| PermissionDecision::Answer(serde_json::from_value(value).unwrap());
    let matching = answer(json!([{"questionId": "q", "selectedOption": "A"}]));
    let unoffered = answer(json!([{"questionId": "q", "selectedOption": "B"}]));

    assert!(super::admits(&PermissionDecision::AllowOnce, &approval));
    assert!(!super::admits(&PermissionDecision::AllowAlways, &approval));
    assert!(!super::admits(&matching, &approval));
    assert!(super::admits(&matching, &question));
    assert!(!super::admits(&unoffered, &question));
    assert!(!super::admits(&PermissionDecision::AllowOnce, &question));
    assert!(super::admits(&PermissionDecision::Deny, &question));
}
