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

/// An optional frame a command may need to write.
pub(crate) type AdapterOptionalEgress = Result<Option<AdapterEgress>, AdapterRuntimeError>;

pub(crate) trait HarnessAdapter {
    fn model_config_hydration(&mut self) -> Result<Option<AdapterEgress>, AdapterRuntimeError> {
        Ok(None)
    }
    fn finish_model_config_hydration(&mut self) {}
    fn provisional_turn_pending(&self) -> bool {
        false
    }
    fn goal_get(&mut self) -> Result<Option<AdapterEgress>, AdapterRuntimeError> {
        Ok(None)
    }
    fn goal_state(&self) -> Option<crate::acp::goal::GoalState> {
        None
    }
    fn goal_pending(&self) -> bool {
        false
    }
    fn native_rpc_pending(&self) -> bool {
        self.goal_pending()
    }
    fn child_read_pending(&self) -> bool {
        false
    }
    fn read_child(
        &mut self,
        _child_thread_id: &str,
        _parent_thread_id: &str,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        Err(AdapterRuntimeError::UnsupportedCommand)
    }
    /// Requests a native fork of the bound session through its latest turn.
    /// Harnesses without a characterised fork method fail closed.
    fn fork_session(&mut self) -> Result<AdapterEgress, AdapterRuntimeError> {
        Err(AdapterRuntimeError::UnsupportedCommand)
    }
    /// Lets go of a native fork this runtime created once its fork Chat is
    /// committed (`committed`) or refused. `Ok(None)` means the harness holds
    /// nothing to release, and the reply is already available.
    fn release_fork(
        &mut self,
        _fork: &crate::acp::intent::ExternalId,
        _committed: bool,
    ) -> AdapterOptionalEgress {
        Err(AdapterRuntimeError::UnsupportedCommand)
    }
    /// A Turn is running: remember the release for the next idle moment and
    /// answer the RPC now. False when the harness cannot defer it.
    fn defer_fork_release(
        &mut self,
        _fork: &crate::acp::intent::ExternalId,
        _committed: bool,
    ) -> bool {
        false
    }
    /// The next deferred fork release, written while the runtime is idle.
    fn deferred_fork_release(&mut self) -> AdapterOptionalEgress {
        Ok(None)
    }
    /// True only for a frame that repeats the terminal of the Turn this
    /// adapter last finished (same Turn and terminal kind), so ingesting it
    /// would change nothing. Harnesses that cannot prove that answer false.
    fn is_duplicate_terminal(&self, _frame: &Frame, _resources: &dyn ResourceMapper) -> bool {
        false
    }
    fn accept_oversized_response(&mut self, _id: u64) -> Result<(), AdapterRuntimeError> {
        Err(AdapterRuntimeError::Frame)
    }
    fn take_native_rpc_reply(&mut self) -> Option<crate::acp::goal::NativeRpcReply> {
        None
    }
    fn mutate_goal(
        &mut self,
        _action: crate::acp::goal::GoalAction,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        Err(AdapterRuntimeError::UnsupportedCommand)
    }
    fn session_config(&self) -> Option<crate::acp::session_config::SessionConfig> {
        None
    }
    fn refusal_detail(&self) -> Option<RefusalDetail> {
        None
    }
    fn config_pending(&self) -> bool {
        false
    }
    fn configure(
        &mut self,
        _intent: &crate::acp::session_config::ConfigIntent,
    ) -> Result<Option<AdapterEgress>, AdapterRuntimeError> {
        Err(AdapterRuntimeError::UnsupportedCommand)
    }
    fn capabilities(&self) -> Option<CapabilitySnapshotV1>;
    fn command(&mut self, command: AdapterCommand) -> Result<AdapterEgress, AdapterRuntimeError>;
    fn cancel_permission(
        &mut self,
        _request_id: &SageServerRequestId,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        Err(AdapterRuntimeError::UnsupportedCommand)
    }
    fn discard_invalid_response(
        &mut self,
        _frame: &Frame,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        Err(AdapterRuntimeError::Frame)
    }
    fn ingest(
        &mut self,
        frame: &Frame,
        resources: &dyn ResourceMapper,
    ) -> Result<AdapterBatch, AdapterRuntimeError>;
    fn commit_mandatory_egress(&mut self, token: &EgressToken) -> Result<(), AdapterRuntimeError>;
    fn resolve_egress(
        &mut self,
        token: EgressToken,
        resolution: EgressResolution,
    ) -> Result<(), AdapterRuntimeError>;
}
