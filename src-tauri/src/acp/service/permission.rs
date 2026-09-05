use crate::acp::{
    contract::{
        RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId, SageServerRequestId,
        SageTurnId,
    },
    lifecycle::ServerRequestState,
    runtime::{HostError, JournalLane, permissions::PermissionDecision},
    scheduler::RuntimeOwner,
    store::{RequestStateCommand, StoreMutation},
};

use super::AcpService;

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
    decision: PermissionDecision,
    state: &'static str,
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
        let state = match decision {
            PermissionDecision::Ask => ServerRequestState::Asked,
            PermissionDecision::AllowOnce => ServerRequestState::AllowPending,
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
        self.runtime
            .for_owner(&owner)
            .map_err(|_| HostError::Intent)?
            .resolve_permission(owner, request_id.clone(), decision)?;
        Ok(PermissionDecisionResponse {
            project_id,
            chat_id,
            session_id,
            turn_id,
            runtime_epoch,
            request_id,
            decision,
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
mod tests;
