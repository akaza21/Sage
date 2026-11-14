use serde::Serialize;

use sage_acp_protocol::contract::{
    ContractError, EntityRevision, ProjectGeneration, RuntimeEpoch, SageAgentSessionId, SageChatId,
    SageProjectId,
};
use sage_core::limits::PREVIEW_LIMITS;

use super::{
    discovery::{PreviewDiscoveryRequest, PreviewTargetRef},
    error::PreviewError,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum PreviewPhase {
    Idle,
    Discovering,
    Starting,
    WaitingForOrigin,
    HealthChecking,
    Active,
    HealthFailed,
    Exited,
    Stopping,
    Stopped,
}

impl PreviewPhase {
    pub(crate) fn allows(self, next: Self) -> bool {
        use PreviewPhase::{
            Active, Discovering, Exited, HealthChecking, HealthFailed, Idle, Starting, Stopped,
            Stopping, WaitingForOrigin,
        };
        matches!(
            (self, next),
            (Idle, Discovering | Starting)
                | (Discovering, Idle)
                | (
                    Starting,
                    WaitingForOrigin | HealthFailed | Exited | Stopping
                )
                | (
                    WaitingForOrigin,
                    HealthChecking | HealthFailed | Exited | Stopping
                )
                | (HealthChecking, Active | HealthFailed | Exited | Stopping)
                | (Active, HealthChecking | Stopping | Exited)
                | (HealthFailed | Exited | Stopped, Discovering | Starting)
                | (Stopping, Stopped)
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum PreviewFailure {
    ScriptMissing,
    OriginNotLoopback,
    OriginUnsupported,
    StartupTimeout,
    HealthTimeout,
    ExitedEarly,
    Conflict,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreviewProjectionOwner {
    pub project_id: SageProjectId,
    pub project_generation: ProjectGeneration,
    pub transition_epoch: u64,
    pub chat_id: SageChatId,
    pub session_id: SageAgentSessionId,
    pub runtime_epoch: RuntimeEpoch,
}

impl PreviewProjectionOwner {
    pub(crate) fn from_discovery(request: &PreviewDiscoveryRequest) -> Self {
        Self {
            project_id: request.project_id.clone(),
            project_generation: request.project_generation,
            transition_epoch: request.expected_transition_epoch,
            chat_id: request.chat_id.clone(),
            session_id: request.session_id.clone(),
            runtime_epoch: request.runtime_epoch,
        }
    }

    pub(crate) fn from_target(target: &PreviewTargetRef) -> Self {
        Self {
            project_id: target.project_id.clone(),
            project_generation: target.project_generation,
            transition_epoch: target.expected_transition_epoch,
            chat_id: target.chat_id.clone(),
            session_id: target.session_id.clone(),
            runtime_epoch: target.runtime_epoch,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreviewProjection {
    #[serde(flatten)]
    pub owner: PreviewProjectionOwner,
    pub preview_revision: EntityRevision,
    pub phase: PreviewPhase,
    pub failure: Option<PreviewFailure>,
    pub target: Option<PreviewTargetRef>,
    pub origin: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreviewProjectionChangedV1 {
    pub kind: PreviewProjectionEventKind,
    pub preview: PreviewProjection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum PreviewProjectionEventKind {
    Preview,
}

impl PreviewProjection {
    pub(crate) fn initial(owner: PreviewProjectionOwner) -> Result<Self, PreviewError> {
        Self::at_revision(owner, EntityRevision::new(1).map_err(contract_error)?)
    }

    pub(crate) fn at_revision(
        owner: PreviewProjectionOwner,
        preview_revision: EntityRevision,
    ) -> Result<Self, PreviewError> {
        if preview_revision.get() > PREVIEW_LIMITS.revision_max {
            return Err(PreviewError::internal("Preview revision is out of range."));
        }
        Ok(Self {
            owner,
            preview_revision,
            phase: PreviewPhase::Idle,
            failure: None,
            target: None,
            origin: None,
        })
    }

    pub(crate) fn advance(
        &self,
        phase: PreviewPhase,
        failure: Option<PreviewFailure>,
        origin: Option<String>,
    ) -> Result<Self, PreviewError> {
        if !self.phase.allows(phase) {
            return Err(PreviewError::conflict(
                "previewPhase",
                "Preview phase transition is not admitted.",
            ));
        }
        let preview_revision = self.next_revision()?;
        Ok(Self {
            owner: self.owner.clone(),
            preview_revision,
            phase,
            failure,
            target: self.target.clone(),
            origin,
        })
    }

    pub(crate) fn with_failure(&self, failure: PreviewFailure) -> Result<Self, PreviewError> {
        Ok(Self {
            owner: self.owner.clone(),
            preview_revision: self.next_revision()?,
            phase: self.phase,
            failure: Some(failure),
            target: self.target.clone(),
            origin: self.origin.clone(),
        })
    }

    fn next_revision(&self) -> Result<EntityRevision, PreviewError> {
        let revision = self.preview_revision.next().map_err(contract_error)?;
        if revision.get() > PREVIEW_LIMITS.revision_max {
            return Err(PreviewError::internal("Preview revision is exhausted."));
        }
        Ok(revision)
    }

    pub(crate) fn event(&self) -> PreviewProjectionChangedV1 {
        PreviewProjectionChangedV1 {
            kind: PreviewProjectionEventKind::Preview,
            preview: self.clone(),
        }
    }
}

fn contract_error(_error: ContractError) -> PreviewError {
    PreviewError::internal("Preview revision is exhausted.")
}
