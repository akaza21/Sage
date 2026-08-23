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

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PermissionDecisionResponse {
    project_id: SageProjectId,
    chat_id: SageChatId,
    session_id: SageAgentSessionId,
    request_id: SageServerRequestId,
    decision: PermissionDecision,
    state: &'static str,
}

impl AcpService {
    pub(crate) fn resolve_permission_decision(
        &self,
        project_id: SageProjectId,
        chat_id: SageChatId,
        session_id: SageAgentSessionId,
        request_id: SageServerRequestId,
        decision: PermissionDecision,
    ) -> Result<PermissionDecisionResponse, HostError> {
        let (epoch, turn_id): (i64, String) = self
            .database
            .connection()
            .query_row(
                "SELECT runtime_epoch,turn_id FROM server_requests
                 WHERE id=?1 AND project_id=?2 AND chat_id=?3 AND session_id=?4 LIMIT 1",
                rusqlite::params![
                    request_id.as_str(),
                    project_id.as_str(),
                    chat_id.as_str(),
                    session_id.as_str()
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|_| HostError::Intent)?;
        let runtime_epoch = RuntimeEpoch::new(u64::try_from(epoch).map_err(|_| HostError::Intent)?)
            .map_err(|_| HostError::Intent)?;
        let turn_id = SageTurnId::parse(turn_id).map_err(|_| HostError::Intent)?;
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
                    turn_id,
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
