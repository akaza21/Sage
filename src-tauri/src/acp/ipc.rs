use serde::{Deserialize, Serialize};

use super::{
    contract::*,
    intent::{BindingId, ModelId, PageCursor, Prompt},
};

pub use super::projection::AcpChatPage;

pub(crate) const ACP_PROJECTION_CHANGED_EVENT: &str = "acp-projection-changed-v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GetAcpChatPageRequest {
    pub project_id: SageProjectId,
    pub project_generation: ProjectGeneration,
    pub chat_id: SageChatId,
    pub cursor: Option<PageCursor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GetCodexDiscoveryRequest {
    pub project_id: SageProjectId,
    pub project_generation: ProjectGeneration,
    pub binding_id: BindingId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CodexDiscoveryAccount {
    ManagedChatgpt,
    SignedOut,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodexDiscoveryModel {
    pub id: ModelId,
    pub name: RequiredText<512>,
    pub is_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodexDiscoveryResponse {
    pub project_id: SageProjectId,
    pub project_generation: ProjectGeneration,
    pub root_id: SageRootId,
    pub binding_id: BindingId,
    pub runtime_epoch: RuntimeEpoch,
    pub account: CodexDiscoveryAccount,
    pub models: Vec<CodexDiscoveryModel>,
    pub default_model_id: Option<ModelId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodexAuthScope {
    pub project_id: SageProjectId,
    pub project_generation: ProjectGeneration,
    pub binding_id: BindingId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CodexAuthPhase {
    SignedOut,
    Waiting,
    Recovery,
    SignedIn,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodexAuthResponse {
    pub project_id: SageProjectId,
    pub project_generation: ProjectGeneration,
    pub binding_id: BindingId,
    pub phase: CodexAuthPhase,
    pub revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartAcpSessionRequest {
    pub project_id: SageProjectId,
    pub project_generation: ProjectGeneration,
    pub chat_id: SageChatId,
    pub expected_projection_revision: ProjectionRevision,
    pub client_intent_id: ClientIntentId,
    pub binding_id: BindingId,
    pub model_id: Option<ModelId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActivateAcpSessionRequest {
    pub project_id: SageProjectId,
    pub project_generation: ProjectGeneration,
    pub chat_id: SageChatId,
    pub expected_projection_revision: ProjectionRevision,
    pub client_intent_id: ClientIntentId,
    pub session_id: SageAgentSessionId,
    pub runtime_epoch: RuntimeEpoch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "target",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum SendAcpTurnRequest {
    Session {
        project_id: SageProjectId,
        project_generation: ProjectGeneration,
        chat_id: SageChatId,
        expected_projection_revision: ProjectionRevision,
        client_intent_id: ClientIntentId,
        session_id: SageAgentSessionId,
        runtime_epoch: RuntimeEpoch,
        prompt: Prompt,
    },
    NewSession {
        project_id: SageProjectId,
        project_generation: ProjectGeneration,
        chat_id: SageChatId,
        expected_projection_revision: ProjectionRevision,
        client_intent_id: ClientIntentId,
        binding_id: BindingId,
        model_id: Option<ModelId>,
        prompt: Prompt,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CancelAcpTurnRequest {
    pub project_id: SageProjectId,
    pub project_generation: ProjectGeneration,
    pub chat_id: SageChatId,
    pub expected_projection_revision: ProjectionRevision,
    pub client_intent_id: ClientIntentId,
    pub session_id: SageAgentSessionId,
    pub runtime_epoch: RuntimeEpoch,
    pub turn_id: SageTurnId,
    pub expected_turn_state: TurnState,
    pub expected_turn_revision: EntityRevision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CloseAcpSessionRequest {
    pub project_id: SageProjectId,
    pub project_generation: ProjectGeneration,
    pub chat_id: SageChatId,
    pub expected_projection_revision: ProjectionRevision,
    pub client_intent_id: ClientIntentId,
    pub session_id: SageAgentSessionId,
    pub runtime_epoch: RuntimeEpoch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpMutationResponse {
    pub projection: AcpChatPage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectionChangedV1 {
    pub project_id: SageProjectId,
    pub chat_id: SageChatId,
    pub session_id: SageAgentSessionId,
    pub projection_revision: ProjectionRevision,
}

impl ProjectionChangedV1 {
    pub fn validate(&self) -> Result<(), IntentError> {
        (self.projection_revision.get() > 0)
            .then_some(())
            .ok_or(IntentError::InvalidProjectionRevision)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AcpUnavailableReason {
    DesktopRuntimeUnavailable,
    BindingUnavailable,
    CapabilityUnavailable,
    PlatformUnsupported,
    SessionUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AcpConflictResource {
    ProjectGeneration,
    ProjectionRevision,
    Session,
    RuntimeEpoch,
    TurnState,
    TurnRevision,
    ClientIntent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AcpCapacityResource {
    ActiveTurn,
    LiveTree,
    History,
    PageRead,
    Storage,
    Queue,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AcpIpcError {
    Unavailable {
        reason: AcpUnavailableReason,
        message: SafeText<8192>,
    },
    Conflict {
        resource: AcpConflictResource,
        message: SafeText<8192>,
    },
    Capacity {
        resource: AcpCapacityResource,
        limit: u64,
        message: SafeText<8192>,
    },
    InvalidRequest {
        message: SafeText<8192>,
    },
    RecoveryRequired {
        message: SafeText<8192>,
    },
    Internal {
        message: SafeText<8192>,
    },
}

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
    // Activation opens the requested next epoch from a cold session. The
    // store owns exact monotonically increasing epoch admission; an already
    // open runtime here is necessarily stale for activation.
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
    // Unrelated ingress can advance the chat projection after the UI has
    // rendered this turn. Cancellation stays safe because its remaining
    // guards bind it to the exact session, runtime, and turn revision.
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
    if snapshot.turn_state != Some(request.expected_turn_state) {
        return Err(IntentError::UnexpectedTurnState);
    }
    if snapshot.turn_revision != Some(request.expected_turn_revision) {
        return Err(IntentError::StaleTurnRevision);
    }
    Ok(())
}
