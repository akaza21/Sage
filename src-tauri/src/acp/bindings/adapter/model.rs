use super::*;

/// Authority needed to adopt work the harness started without a prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HarnessTurnAuthority {
    SessionMode(&'static str),
    /// Only user questions; adoption grants no model or tool authority.
    Questions,
    /// Runs under this access, which the harness keeps from the last prompt
    /// Sage sent; it opens only while the chat allows that access.
    Access(sage_acp_protocol::access_mode::SessionAccessMode),
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct AdapterIdentity {
    pub(crate) binding_digest: [u8; 32],
    pub(crate) runtime_epoch: RuntimeEpoch,
    pub(crate) session_id: SageAgentSessionId,
    pub(crate) external_id: ExternalId,
    pub(crate) turn_id: Option<ExternalId>,
    pub(crate) turn_client_message_id: Option<RequiredText<512>>,
}

impl AdapterIdentity {
    /// True only for the exact session identity: same binding, epoch, Sage
    /// session and harness session, and no Turn correlation at all.
    pub(crate) fn is_exact_session(
        &self,
        binding_digest: &[u8; 32],
        runtime_epoch: RuntimeEpoch,
        session_id: &SageAgentSessionId,
        external_session_id: &ExternalId,
    ) -> bool {
        &self.binding_digest == binding_digest
            && self.runtime_epoch == runtime_epoch
            && &self.session_id == session_id
            && &self.external_id == external_session_id
            && self.turn_id.is_none()
            && self.turn_client_message_id.is_none()
    }
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
    #[error(
        "The harness approval bridge did not initialise or its handshake did not match. No prompt was sent."
    )]
    ApprovalBridgeUnavailable,
    #[error(
        "The pending undo could not be applied; activation stopped and the rewind remains pending."
    )]
    RewindUnavailable,
    #[error("the harness reports that it is not signed in")]
    SignInRequired,
    /// The harness took the prompt into a Turn of its own that runs under
    /// access other than the prompt's; it cannot be declined, so the runtime
    /// stops and the prompt's Turn fails.
    #[error("the harness took the prompt into a Turn running under other access")]
    HarnessTurnRefused,
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

pub(crate) type AdapterOptionalEgress = Result<Option<AdapterEgress>, AdapterRuntimeError>;

pub(crate) trait HarnessAdapter {
    /// Only known catalog fields may use the raw catalog ceiling. Bindings with
    /// request/reply protocols must correlate the response before declaring its path.
    fn command_catalog_path(&self, frame: &Frame) -> crate::acp::transport::CommandWirePath {
        crate::acp::transport::CommandWirePath::for_frame(frame.as_bytes())
    }
    fn validate_ingress(
        &self,
        frame: &Frame,
    ) -> Result<(), crate::acp::transport::StructuralError> {
        crate::acp::transport::validate_structure_with_catalog(frame, || {
            self.command_catalog_path(frame)
        })
    }

    fn foreground_turn(&self) -> Option<crate::acp::service::chat_title::ForegroundLease> {
        None
    }
    fn title_completed(&mut self, _intent: &crate::acp::runtime::PumpIntent) {}
    fn start_title(&mut self, _input: &str) -> Result<(), AdapterRuntimeError> {
        Err(AdapterRuntimeError::UnsupportedCommand)
    }
    fn poll_title(&mut self) -> Option<AdapterEgress> {
        None
    }
    fn cancel_title(&mut self) {}
    fn title_pending(&self) -> bool {
        false
    }
    /// Failed cleanup requires the existing supervisor to prove process exit.
    fn title_requires_teardown(&self) -> bool {
        false
    }
    /// `Some(None)` is a real, terminal failure. Intentional foreground
    /// cancellation finishes without a result, so the scheduler can defer it.
    fn take_title_result(&mut self) -> Option<Option<String>> {
        None
    }

    fn receive_command_update(&mut self, _frame: &Frame) -> Result<(), AdapterRuntimeError> {
        Ok(())
    }
    fn discover_commands(&mut self) -> Result<Option<AdapterEgress>, AdapterRuntimeError> {
        Ok(None)
    }
    /// Re-read the harness catalog; bindings use their existing request accounting.
    fn refresh_commands(&mut self) -> AdapterOptionalEgress {
        Err(AdapterRuntimeError::UnsupportedCommand)
    }
    fn available_commands(&self) -> std::sync::Arc<crate::acp::slash_commands::CommandState> {
        crate::acp::slash_commands::CommandState::unsupported()
    }
    fn model_config_hydration(&mut self) -> Result<Option<AdapterEgress>, AdapterRuntimeError> {
        Ok(None)
    }
    fn finish_model_config_hydration(&mut self) {}
    /// The session is still reading what it offers, so config options it
    /// does not report yet may follow.
    fn config_hydrating(&self) -> bool {
        false
    }
    fn provisional_turn_pending(&self) -> bool {
        false
    }
    /// The active Turn is admitted but the harness has not named it, so a
    /// cancel cannot address it yet: hold the cancel and send it once the
    /// Turn binds. False when the harness has nothing to hold it for.
    fn defer_cancel(&mut self) -> bool {
        false
    }
    /// Deadline of retained requests that have not opened a visible card yet.
    fn next_request_deadline(&self) -> Option<std::time::Instant> {
        None
    }
    fn poll_request_deadlines(
        &mut self,
        _resources: &dyn ResourceMapper,
    ) -> Result<Option<AdapterBatch>, AdapterRuntimeError> {
        Ok(None)
    }
    fn goal_get(&mut self) -> Result<Option<AdapterEgress>, AdapterRuntimeError> {
        Ok(None)
    }
    fn goal_state(&self) -> Option<&crate::acp::goal::GoalState> {
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
    fn native_read(
        &mut self,
        _read: &crate::acp::goal::NativeRead,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        Err(AdapterRuntimeError::UnsupportedCommand)
    }
    /// Harnesses without a characterised fork method fail closed.
    fn fork_session(&mut self) -> Result<AdapterEgress, AdapterRuntimeError> {
        Err(AdapterRuntimeError::UnsupportedCommand)
    }
    /// `Ok(None)` means the harness holds nothing to release, and the reply is already available.
    fn release_fork(
        &mut self,
        _fork: &sage_acp_protocol::intent::ExternalId,
        _committed: bool,
    ) -> AdapterOptionalEgress {
        Err(AdapterRuntimeError::UnsupportedCommand)
    }
    /// A Turn is running: remember the release for the next idle moment and
    /// answer the RPC now. False when the harness cannot defer it.
    fn defer_fork_release(
        &mut self,
        _fork: &sage_acp_protocol::intent::ExternalId,
        _committed: bool,
    ) -> bool {
        false
    }
    /// The next deferred fork release, written while the runtime is idle.
    fn deferred_fork_release(&mut self) -> AdapterOptionalEgress {
        Ok(None)
    }
    /// True only when ingesting `frames` in order would read each one as a
    /// repeat of the terminal of the Turn this adapter last finished, and
    /// would leave the adapter unchanged. Harnesses that cannot prove that
    /// answer false.
    fn frames_repeat_terminal(&self, _frames: &[Frame], _resources: &dyn ResourceMapper) -> bool {
        false
    }
    fn accept_oversized_response(&mut self, _id: u64) -> Result<(), AdapterRuntimeError> {
        Err(AdapterRuntimeError::Frame)
    }
    /// Whether an oversized frame without a request id (such as a large
    /// tool result) is skipped rather than failing the session.
    fn skips_oversize_frames(&self) -> bool {
        false
    }
    /// An oversized frame without a request id was read past and dropped.
    fn oversize_frame_skipped(&mut self) {}
    /// Answers a session control the harness has already carried out
    /// without writing a frame: the answer waits in the native RPC reply.
    /// A control done once, such as an undo whose outcome Sage could not
    /// record, may then be confirmed again.
    fn answered_control(
        &mut self,
        _control: &sage_acp_protocol::session_control::SessionControl,
    ) -> bool {
        false
    }
    fn session_control(
        &mut self,
        _control: &sage_acp_protocol::session_control::SessionControl,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        Err(AdapterRuntimeError::UnsupportedCommand)
    }
    fn take_native_rpc_reply(&mut self) -> Option<crate::acp::goal::NativeRpcReply> {
        None
    }
    fn usage(&self) -> Option<std::sync::Arc<sage_acp_protocol::usage::UsageCell>> {
        None
    }
    fn read_usage(&mut self) -> Result<AdapterEgress, AdapterRuntimeError> {
        Err(AdapterRuntimeError::UnsupportedCommand)
    }
    fn abandon_passive_read(&mut self) {}
    fn background_terminals(
        &mut self,
        _action: &crate::acp::background_terminals::TerminalAction,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        Err(AdapterRuntimeError::UnsupportedCommand)
    }
    /// A command-backed goal uses the normal command-turn admission path.
    /// True means a bounded GoalCommand reply is ready without wire egress.
    fn prepare_goal_command(
        &mut self,
        _action: &crate::acp::goal::GoalAction,
    ) -> Result<bool, AdapterRuntimeError> {
        Ok(false)
    }
    fn mutate_goal(
        &mut self,
        _action: crate::acp::goal::GoalAction,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        Err(AdapterRuntimeError::UnsupportedCommand)
    }
    fn session_config(&self) -> Option<sage_acp_protocol::session_config::SessionConfig> {
        None
    }
    fn refusal_detail(&self) -> Option<RefusalDetail> {
        None
    }
    /// Why `prompt`, asked under `access`, must not be sent: checked before
    /// its Turn is prepared, so the chat is told and the session runs on.
    fn prompt_refusal(
        &self,
        _prompt: &crate::acp::transcript_quote::Prompt,
        _access: sage_acp_protocol::access_mode::SessionAccessMode,
    ) -> Option<&'static str> {
        None
    }
    fn config_pending(&self) -> bool {
        false
    }
    fn configure(
        &mut self,
        _intent: &sage_acp_protocol::session_config::ConfigIntent,
    ) -> Result<Option<AdapterEgress>, AdapterRuntimeError> {
        Err(AdapterRuntimeError::UnsupportedCommand)
    }
    fn capabilities(&self) -> Option<CapabilitySnapshotV1>;
    /// What `capabilities` advertises now that the live command catalog may
    /// have changed. `None` for a harness that derives nothing from its
    /// catalog: its negotiated capabilities never move after initialization.
    fn catalog_capabilities(&self) -> Option<CapabilitySnapshotV1> {
        None
    }
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
    /// Each reply of the active Turn still streaming, shown whole, once the
    /// harness's output has ended and nothing more of it can arrive.
    fn unfinished_replies(&mut self) -> Result<Vec<AdapterEvent>, AdapterRuntimeError>;
    /// Output the last batch held back because it filled up, as much as fits
    /// in one batch; the pump takes it until `None` before the next frame.
    fn held_batch(&mut self) -> Result<Option<AdapterBatch>, AdapterRuntimeError>;
    /// Names the Turn the last `AdapterEvent::TurnStarted` announced and
    /// returns what the harness reported for it so far.
    fn adopt_turn(
        &mut self,
        _client_message_id: RequiredText<512>,
        _resources: &dyn ResourceMapper,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        Err(AdapterRuntimeError::UnsupportedCommand)
    }
    /// The harness session mode the Turn the last `AdapterEvent::TurnStarted`
    /// announced runs in.
    fn harness_turn_mode(&self) -> Option<&'static str> {
        None
    }
    fn harness_turn_authority(&self) -> Option<super::HarnessTurnAuthority> {
        self.harness_turn_mode()
            .map(super::HarnessTurnAuthority::SessionMode)
    }
    /// What the harness reports once the answers Sage wrote reached it: a
    /// Turn that only waited on them ends. None when nothing changed.
    fn answers_delivered(
        &mut self,
        _resources: &dyn ResourceMapper,
    ) -> Result<Option<AdapterBatch>, AdapterRuntimeError> {
        Ok(None)
    }
    /// Refuses the Turn the last `AdapterEvent::TurnStarted` announced: what
    /// it asked is declined, and its run is stopped and read to its end
    /// without being shown. Returns the frames that do so.
    fn decline_turn(&mut self) -> Result<Vec<AdapterEgress>, AdapterRuntimeError> {
        Err(AdapterRuntimeError::UnsupportedCommand)
    }
    fn commit_mandatory_egress(&mut self, token: &EgressToken) -> Result<(), AdapterRuntimeError>;
    fn resolve_egress(
        &mut self,
        token: EgressToken,
        resolution: EgressResolution,
    ) -> Result<(), AdapterRuntimeError>;
}
