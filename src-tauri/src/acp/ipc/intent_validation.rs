use crate::acp::{
    contract::{
        EntityRevision, ProjectGeneration, ProjectionRevision, RuntimeEpoch, SageAgentSessionId,
        SageChatId, SageProjectId, SageTurnId,
    },
    lifecycle::TurnState,
};

use super::{ActivateAcpSessionRequest, CancelAcpTurnRequest, SteerAcpTurnRequest};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntentSnapshot {
    pub project_id: SageProjectId,
    pub project_generation: ProjectGeneration,
    pub chat_id: SageChatId,
    pub projection_revision: ProjectionRevision,
    pub session_id: Option<SageAgentSessionId>,
    pub runtime_epoch: Option<RuntimeEpoch>,
    pub turn_id: Option<SageTurnId>,
    pub turn_state: Option<TurnState>,
    pub turn_revision: Option<EntityRevision>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum IntentError {
    #[error("intent belongs to a different project")]
    ForeignProject,
    #[error("project generation is stale")]
    StaleProjectGeneration,
    #[error("intent belongs to a different chat")]
    ForeignChat,
    #[error("projection revision is stale")]
    StaleProjectionRevision,
    #[error("intent belongs to a different session")]
    ForeignSession,
    #[error("runtime epoch is stale")]
    StaleRuntimeEpoch,
    #[error("intent belongs to a different turn")]
    ForeignTurn,
    #[error("turn state changed")]
    UnexpectedTurnState,
    #[error("turn revision is stale")]
    StaleTurnRevision,
    #[error("projection notification revision must be positive")]
    InvalidProjectionRevision,
}

pub fn validate_base_intent(
    project_id: &SageProjectId,
    project_generation: ProjectGeneration,
    chat_id: &SageChatId,
    expected_projection_revision: ProjectionRevision,
    snapshot: &IntentSnapshot,
) -> Result<(), IntentError> {
    if project_id != &snapshot.project_id {
        return Err(IntentError::ForeignProject);
    }
    if project_generation != snapshot.project_generation {
        return Err(IntentError::StaleProjectGeneration);
    }
    if chat_id != &snapshot.chat_id {
        return Err(IntentError::ForeignChat);
    }
    if expected_projection_revision != snapshot.projection_revision {
        return Err(IntentError::StaleProjectionRevision);
    }
    Ok(())
}

pub fn validate_session_intent(
    request: &ActivateAcpSessionRequest,
    snapshot: &IntentSnapshot,
) -> Result<(), IntentError> {
    validate_base_intent(
        &request.project_id,
        request.project_generation,
        &request.chat_id,
        request.expected_projection_revision,
        snapshot,
    )?;
    if snapshot.session_id.as_ref() != Some(&request.session_id) {
        return Err(IntentError::ForeignSession);
    }
    if snapshot.runtime_epoch.is_some() {
        return Err(IntentError::StaleRuntimeEpoch);
    }
    Ok(())
}

pub fn validate_cancel_intent(
    request: &CancelAcpTurnRequest,
    snapshot: &IntentSnapshot,
) -> Result<(), IntentError> {
    if request.project_id != snapshot.project_id {
        return Err(IntentError::ForeignProject);
    }
    if request.project_generation != snapshot.project_generation {
        return Err(IntentError::StaleProjectGeneration);
    }
    if request.chat_id != snapshot.chat_id {
        return Err(IntentError::ForeignChat);
    }
    if request.expected_projection_revision > snapshot.projection_revision {
        return Err(IntentError::StaleProjectionRevision);
    }
    if snapshot.session_id.as_ref() != Some(&request.session_id) {
        return Err(IntentError::ForeignSession);
    }
    if snapshot.runtime_epoch != Some(request.runtime_epoch) {
        return Err(IntentError::StaleRuntimeEpoch);
    }
    if snapshot.turn_id.as_ref() != Some(&request.turn_id) {
        return Err(IntentError::ForeignTurn);
    }
    if !snapshot
        .turn_state
        .is_some_and(|state| state.can_transition_to(TurnState::Cancelling))
    {
        return Err(IntentError::UnexpectedTurnState);
    }
    if snapshot
        .turn_revision
        .is_none_or(|revision| revision < request.expected_turn_revision)
    {
        return Err(IntentError::StaleTurnRevision);
    }
    Ok(())
}

pub fn validate_steer_intent(
    request: &SteerAcpTurnRequest,
    snapshot: &IntentSnapshot,
) -> Result<(), IntentError> {
    if request.project_id != snapshot.project_id {
        return Err(IntentError::ForeignProject);
    }
    if request.project_generation != snapshot.project_generation {
        return Err(IntentError::StaleProjectGeneration);
    }
    if request.chat_id != snapshot.chat_id {
        return Err(IntentError::ForeignChat);
    }
    if request.expected_projection_revision > snapshot.projection_revision {
        return Err(IntentError::StaleProjectionRevision);
    }
    if snapshot.session_id.as_ref() != Some(&request.session_id) {
        return Err(IntentError::ForeignSession);
    }
    if snapshot.runtime_epoch != Some(request.runtime_epoch) {
        return Err(IntentError::StaleRuntimeEpoch);
    }
    if snapshot.turn_id.as_ref() != Some(&request.turn_id) {
        return Err(IntentError::ForeignTurn);
    }
    if !matches!(
        snapshot.turn_state,
        Some(
            TurnState::Sending
                | TurnState::Running
                | TurnState::WaitingModel
                | TurnState::WaitingTool
                | TurnState::WaitingApproval
                | TurnState::WaitingUser
        )
    ) {
        return Err(IntentError::UnexpectedTurnState);
    }
    if snapshot
        .turn_revision
        .is_none_or(|revision| revision < request.expected_turn_revision)
    {
        return Err(IntentError::StaleTurnRevision);
    }
    Ok(())
}
