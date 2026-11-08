use serde::{Deserialize, Serialize};

use super::{
    attached_context::AttachedContexts,
    contract::*,
    intent::{BindingId, ModelId, PageCursor, Prompt},
};

mod intent_validation;

pub use intent_validation::{
    IntentError, IntentSnapshot, validate_base_intent, validate_cancel_intent,
    validate_session_intent, validate_steer_intent,
};

pub use super::projection::AcpChatPage;
#[cfg(test)]
pub(crate) use super::runtime::notice_routes::MAX_PROJECTION_NOTICE_ROUTES;
pub(crate) use super::runtime::notice_routes::{ProjectionNoticeRoutes, route_projection_notice};

pub(crate) const ACP_PROJECTION_CHANGED_EVENT: &str = "acp-projection-changed-v1";
/// Only child transcripts changed: open Subagent views re-read, and the
/// parent page, whose projection revision did not move, is not reloaded.
pub(crate) const ACP_CHILD_PROJECTION_CHANGED_EVENT: &str = "acp-child-projection-changed-v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChildProjectionChangedV1 {
    pub project_id: SageProjectId,
    pub chat_id: SageChatId,
    pub session_id: SageAgentSessionId,
    pub child_projection_epoch: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GetAcpChatPageRequest {
    pub project_id: SageProjectId,
    pub project_generation: ProjectGeneration,
    pub chat_id: SageChatId,
    pub cursor: Option<PageCursor>,
}

/// Narrow owner-bound immutable snapshot read. Returns one bounded pair,
/// never raw event JSON and never a history-page payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GetAcpFileSnapshotRequest {
    pub project_id: SageProjectId,
    pub project_generation: ProjectGeneration,
    pub expected_projection_revision: ProjectionRevision,
    pub chat_id: SageChatId,
    pub session_id: SageAgentSessionId,
    pub runtime_epoch: RuntimeEpoch,
    pub turn_id: SageTurnId,
    pub item_id: SageItemId,
    pub diff_ordinal: u64,
    #[serde(with = "crate::acp::file_diff::wire_counter")]
    pub snapshot_revision: u64,
    pub root_id: SageRootId,
    pub relative_path: String,
    pub content_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpFileSnapshotResponse {
    pub project_id: SageProjectId,
    pub projection_revision: ProjectionRevision,
    pub chat_id: SageChatId,
    pub session_id: SageAgentSessionId,
    pub runtime_epoch: RuntimeEpoch,
    pub turn_id: SageTurnId,
    pub item_id: SageItemId,
    pub diff_ordinal: u64,
    pub root_id: SageRootId,
    pub relative_path: String,
    #[serde(with = "crate::acp::file_diff::wire_counter")]
    pub snapshot_revision: u64,
    pub old_text: String,
    pub new_text: String,
    pub old_bytes: u64,
    pub new_bytes: u64,
    pub content_digest: String,
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
    ProviderManaged,
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

/// Which staged prompt files a send carries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum StagedSelection {
    /// The chat's staged batch, only while it is still at this revision.
    Revision(String),
    /// Retry of this turn: the files its prompt carried, only while they
    /// are still held for it.
    RetryOf(SageTurnId),
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
        #[serde(default)]
        attached_context: AttachedContexts,
        /// The staged prompt files this send carries. A send that names none
        /// leaves the chat's staged batch for the send that does.
        #[serde(default)]
        staged: Option<StagedSelection>,
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
        #[serde(default)]
        attached_context: AttachedContexts,
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
pub struct SteerAcpTurnRequest {
    pub project_id: SageProjectId,
    pub project_generation: ProjectGeneration,
    pub chat_id: SageChatId,
    pub expected_projection_revision: ProjectionRevision,
    pub client_intent_id: ClientIntentId,
    pub session_id: SageAgentSessionId,
    pub runtime_epoch: RuntimeEpoch,
    pub turn_id: SageTurnId,
    pub expected_turn_revision: EntityRevision,
    pub prompt: Prompt,
    #[serde(default)]
    pub attached_context: AttachedContexts,
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
    /// Child-transcript invalidation epoch (see `AcpChatPage`). It never
    /// moves `projection_revision`, the optimistic-concurrency token.
    #[serde(default, skip_serializing_if = "is_zero_epoch")]
    pub child_projection_epoch: u64,
}

fn is_zero_epoch(epoch: &u64) -> bool {
    *epoch == 0
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
    ImageInputUnsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AcpConflictResource {
    ConfigRevision,
    ProjectGeneration,
    ProjectionRevision,
    Session,
    RuntimeEpoch,
    TurnState,
    TurnRevision,
    ClientIntent,
    SteerOutcomeUnknown,
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
    Rejected {
        message: SafeText<8192>,
    },
    SignInRequired {
        message: SafeText<8192>,
    },
    RecoveryRequired {
        message: SafeText<8192>,
    },
    Internal {
        message: SafeText<8192>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PromptDispatch {
    NotStarted,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendAcpTurnFailure {
    #[serde(flatten)]
    pub failure: AcpIpcError,
    pub prompt_dispatch: PromptDispatch,
}
