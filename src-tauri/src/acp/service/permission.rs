use crate::acp::{
    contract::{
        RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId, SageServerRequestId,
        SageTurnId,
    },
    intent::{NormalizedPayload, PermissionOptionKind},
    lifecycle::ServerRequestState,
    runtime::{HostError, JournalLane, permissions::PermissionDecision},
    scheduler::RuntimeOwner,
    store::{RequestStateCommand, StoreMutation},
};

use super::AcpService;
use rusqlite::OptionalExtension;

pub(crate) struct PermissionDecisionIntent {
    pub(crate) project_id: SageProjectId,
    pub(crate) chat_id: SageChatId,
    pub(crate) session_id: SageAgentSessionId,
    pub(crate) turn_id: SageTurnId,
    pub(crate) runtime_epoch: RuntimeEpoch,
    pub(crate) request_id: SageServerRequestId,
    pub(crate) decision: PermissionDecision,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PermissionDecisionResponse {
    project_id: SageProjectId,
    chat_id: SageChatId,
    session_id: SageAgentSessionId,
    turn_id: SageTurnId,
    runtime_epoch: RuntimeEpoch,
    request_id: SageServerRequestId,
    decision: &'static str,
    state: &'static str,
}

/// Whether `decision` may resolve the pending request whose normalized
/// payload is `payload`. Ask and Deny resolve any request; an allow needs a
/// matching offered approval option, and answers must answer exactly the
/// questions asked.
fn admits(decision: &PermissionDecision, payload: &NormalizedPayload) -> bool {
    let NormalizedPayload::PermissionRequest {
        offered_option_kinds,
        user_input,
        ..
    } = payload
    else {
        return false;
    };
    let offered = |kind| {
        user_input.is_none()
            && offered_option_kinds
                .as_ref()
                .is_some_and(|kinds| kinds.as_slice().contains(&kind))
    };
    match decision {
        PermissionDecision::Ask | PermissionDecision::Deny => true,
        PermissionDecision::AllowOnce => offered(PermissionOptionKind::AllowOnce),
        PermissionDecision::AllowAlways => offered(PermissionOptionKind::AllowAlways),
        PermissionDecision::Answer(answers) => user_input
            .as_ref()
            .is_some_and(|request| request.admit(answers).is_ok()),
    }
}

impl AcpService {
    pub(crate) fn resolve_permission_decision(
        &self,
        intent: PermissionDecisionIntent,
    ) -> Result<PermissionDecisionResponse, HostError> {
        let PermissionDecisionIntent {
            project_id,
            chat_id,
            session_id,
            turn_id,
            runtime_epoch,
            request_id,
            decision,
        } = intent;
        if decision.resolves_allowed() {
            let epoch = i64::try_from(runtime_epoch.get()).map_err(|_| HostError::Intent)?;
            let payload_json: Option<String> = self.database.connection().query_row(
                "SELECT normalized_payload_json FROM server_requests WHERE id=?1 AND project_id=?2 AND chat_id=?3 AND session_id=?4 AND turn_id=?5 AND runtime_epoch=?6 AND kind='permission' AND state IN ('received','asked')",
                rusqlite::params![request_id.as_str(), project_id.as_str(), chat_id.as_str(), session_id.as_str(), turn_id.as_str(), epoch],
                |row| row.get(0),
            ).optional().map_err(|_| HostError::RecoveryRequired)?;
            let admitted = payload_json
                .as_deref()
                .and_then(|json| serde_json::from_str::<NormalizedPayload>(json).ok())
                .is_some_and(|payload| admits(&decision, &payload));
            if !admitted {
                return Err(HostError::Intent);
            }
        }
        let label = decision.label();
        tracing::info!(
            acp_request_resolved = request_id.as_str(),
            decision = label,
            "user resolved a harness request"
        );
        let state = match decision {
            PermissionDecision::Ask => ServerRequestState::Asked,
            PermissionDecision::AllowOnce
            | PermissionDecision::AllowAlways
            | PermissionDecision::Answer(_) => ServerRequestState::AllowPending,
            PermissionDecision::Deny => ServerRequestState::DeclinePending,
        };
        self.journal
            .enqueue_one(
                JournalLane::Control,
                StoreMutation::RequestState(RequestStateCommand {
                    project_id: project_id.clone(),
                    chat_id: chat_id.clone(),
                    session_id: session_id.clone(),
                    turn_id: turn_id.clone(),
                    runtime_epoch,
                    request_id: request_id.clone(),
                    state,
                    now_millis: crate::persistence::now_millis(),
                }),
                u64::try_from(crate::persistence::now_millis())
                    .unwrap_or_default()
                    .saturating_add(crate::acp::limits::LIMITS.deadlines_ms.ordinary_rpc),
            )
            .and_then(|ticket| ticket.blocking_committed())
            .map_err(|_| HostError::RecoveryRequired)?;
        let owner = RuntimeOwner {
            project_id: project_id.clone(),
            chat_id: chat_id.clone(),
            session_id: session_id.clone(),
            runtime_epoch,
        };
        self.host_for(&owner)
            .map_err(|_| HostError::Intent)?
            .resolve_permission(owner, request_id.clone(), decision)?;
        Ok(PermissionDecisionResponse {
            project_id,
            chat_id,
            session_id,
            turn_id,
            runtime_epoch,
            request_id,
            decision: label,
            state: match state {
                ServerRequestState::Asked => "asked",
                ServerRequestState::AllowPending => "allowPending",
                ServerRequestState::DeclinePending => "declinePending",
                _ => unreachable!("permission decision target is fixed"),
            },
        })
    }
}

#[cfg(test)]
pub(super) mod tests;
