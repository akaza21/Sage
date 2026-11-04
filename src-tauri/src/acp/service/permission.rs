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
        if matches!(
            decision,
            PermissionDecision::AllowOnce | PermissionDecision::AllowAlways
        ) {
            let epoch = i64::try_from(runtime_epoch.get()).map_err(|_| HostError::Intent)?;
            let payload_json: Option<String> = self.database.connection().query_row(
                "SELECT normalized_payload_json FROM server_requests WHERE id=?1 AND project_id=?2 AND chat_id=?3 AND session_id=?4 AND turn_id=?5 AND runtime_epoch=?6 AND kind='permission' AND state IN ('received','asked')",
                rusqlite::params![request_id.as_str(), project_id.as_str(), chat_id.as_str(), session_id.as_str(), turn_id.as_str(), epoch],
                |row| row.get(0),
            ).optional().map_err(|_| HostError::RecoveryRequired)?;
            let allowed = payload_json
                .as_deref()
                .and_then(|json| serde_json::from_str::<NormalizedPayload>(json).ok())
                .is_some_and(|payload| {
                    matches!(payload,
                    NormalizedPayload::PermissionRequest { offered_option_kinds: Some(kinds), .. }
                    if kinds.as_slice().contains(&match decision {
                        PermissionDecision::AllowOnce => PermissionOptionKind::AllowOnce,
                        PermissionDecision::AllowAlways => PermissionOptionKind::AllowAlways,
                        _ => unreachable!(),
                    }))
                });
            if !allowed {
                return Err(HostError::Intent);
            }
        }
        let state = match decision {
            PermissionDecision::Ask => ServerRequestState::Asked,
            PermissionDecision::AllowOnce | PermissionDecision::AllowAlways => {
                ServerRequestState::AllowPending
            }
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
pub(super) mod tests;
