use super::*;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct AdapterIdentity {
    pub(crate) binding_digest: [u8; 32],
    pub(crate) runtime_epoch: RuntimeEpoch,
    pub(crate) session_id: SageAgentSessionId,
    pub(crate) external_id: ExternalId,
    pub(crate) turn_id: Option<ExternalId>,
    pub(crate) turn_client_message_id: Option<RequiredText<512>>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct AdapterModel {
    pub(crate) id: ModelId,
    pub(crate) name: RequiredText<512>,
    pub(crate) is_default: bool,
}

impl AdapterModel {
    pub(crate) fn actual_bytes(&self) -> Option<usize> {
        self.id.as_str().len().checked_add(self.name.as_str().len())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum AdapterRuntimeError {
    #[error("adapter operation is unavailable or invalid in the current state")]
    Operation,
    #[error("the harness rejected the correlated session configuration mutation")]
    ConfigMutationRejected,
    #[error("the harness rejected the correlated native Goal mutation")]
    GoalMutationRejected,
    #[error("the adapter does not advertise this command")]
    UnsupportedCommand,
    #[error("there is no exact pending permission intent for this decision")]
    PermissionIntent,
    #[error("there is no exact pending request for this later permission decision")]
    LaterPermissionMiss,
    #[error("adapter launch artifact is not admitted")]
    LaunchUnavailable,
    #[error("the harness reports that it is not signed in")]
    SignInRequired,
    #[error("adapter frame is invalid")]
    Frame,
    #[error("adapter event batch exceeds its count limit")]
    BatchCount,
    #[error("adapter event batch exceeds its actual-byte limit")]
    BatchBytes,
    #[error("mandatory deny egress failed closed and requires teardown")]
    MandatoryDenyTeardown,
    #[error("egress resolution does not match the reserved adapter intent")]
    EgressIntent,
}
