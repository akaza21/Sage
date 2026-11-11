use super::*;
use crate::acp::{
    bindings::adapter::{
        AdapterBatch, AdapterCommand, AdapterEgress, AdapterOptionalEgress, AdapterRuntimeError,
        EgressResolution, EgressToken, HarnessAdapter, ResourceMapper,
    },
    intent::ExternalId,
    lifecycle::ServerRequestState,
    runtime::{later_permission::resolve_later_permission, permissions::PermissionDecision},
    transport::{Frame, OutboundFrame},
};

use super::super::{request::RequestKind, state::CodexAdapter};

impl HarnessAdapter for CodexRuntime {
    fn frames_repeat_terminal(&self, frames: &[Frame], resources: &dyn ResourceMapper) -> bool {
        self.ingests_as_duplicate_terminals(frames, resources)
    }
    fn discard_invalid_response(
        &mut self,
        frame: &Frame,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        self.discard_invalid_model_page(frame)
    }
    fn available_commands(&self) -> std::sync::Arc<crate::acp::slash_commands::CommandState> {
        self.inner.skills()
    }
    /// Reads the skills while idle, first and after Codex says they changed.
    fn discover_commands(&mut self) -> Result<Option<AdapterEgress>, AdapterRuntimeError> {
        let wanted = self.inner.skills_wanted();
        wanted
            .then(|| self.request(CodexAdapter::skills_list))
            .transpose()
    }
    fn model_config_hydration(&mut self) -> Result<Option<AdapterEgress>, AdapterRuntimeError> {
        Ok(self.request_model_config())
    }
    fn finish_model_config_hydration(&mut self) {
        self.model_catalog_idle_only = true;
    }
    fn session_config(&self) -> Option<crate::acp::session_config::SessionConfig> {
        self.session_config.clone()
    }
    fn config_hydrating(&self) -> bool {
        self.catalogue_pending()
    }

    fn configure(
        &mut self,
        intent: &crate::acp::session_config::ConfigIntent,
    ) -> Result<Option<AdapterEgress>, AdapterRuntimeError> {
        self.configure_thread(intent).map(|()| None)
    }
    fn provisional_turn_pending(&self) -> bool {
        self.inner.has_provisional_turn() || self.inner.has_action_handoff()
    }
    fn defer_cancel(&mut self) -> bool {
        self.inner.defer_cancel()
    }
    fn goal_get(&mut self) -> Result<Option<AdapterEgress>, AdapterRuntimeError> {
        if self.inner.goal_supported() {
            match self.request(CodexAdapter::goal_get) {
                Ok(egress) => Ok(Some(egress)),
                Err(error) => {
                    self.inner.goal_unavailable();
                    Err(error)
                }
            }
        } else {
            Ok(None)
        }
    }

    fn goal_state(&self) -> Option<crate::acp::goal::GoalState> {
        Some(self.inner.goal_state().clone())
    }

    fn goal_pending(&self) -> bool {
        self.inner.goal_mutation_pending()
    }

    fn native_rpc_pending(&self) -> bool {
        self.inner.goal_mutation_pending()
            || self.inner.child_read_pending()
            || self.inner.fork_rpc_pending()
            || self.inner.rate_limits_read_pending()
            || self.inner.revert_pending()
    }

    fn usage(&self) -> Option<std::sync::Arc<crate::acp::usage::UsageCell>> {
        Some(std::sync::Arc::clone(&self.usage))
    }

    fn read_usage(&mut self) -> Result<AdapterEgress, AdapterRuntimeError> {
        self.native_rpc_reply = None;
        self.request(CodexAdapter::rate_limits_read)
    }

    fn abandon_usage_read(&mut self) {
        self.inner.abandon_rate_limits_read();
    }

    /// Each read whose reply may be past the frame bound keeps the decoder
    /// in recoverable mode.
    fn child_read_pending(&self) -> bool {
        self.inner.oversize_recoverable()
    }

    fn session_control(
        &mut self,
        control: &crate::acp::session_control::SessionControl,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        self.control_request(control)
    }

    fn fork_session(&mut self) -> Result<AdapterEgress, AdapterRuntimeError> {
        self.fork_request()
    }

    fn release_fork(&mut self, fork: &ExternalId, done: bool) -> AdapterOptionalEgress {
        self.release_fork_request(fork, done)
    }
    fn defer_fork_release(&mut self, fork: &ExternalId, done: bool) -> bool {
        self.defer_fork_release_request(fork, done)
    }
    fn deferred_fork_release(&mut self) -> AdapterOptionalEgress {
        self.deferred_fork_release_request()
    }

    fn read_child(
        &mut self,
        child_thread_id: &str,
        parent_thread_id: &str,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        self.native_rpc_reply = None;
        self.request(|adapter, id| adapter.child_read(id, child_thread_id, parent_thread_id))
    }

    fn accept_oversized_response(&mut self, id: u64) -> Result<(), AdapterRuntimeError> {
        if self.inner.skills_oversized(id) {
            return Ok(());
        }
        let native = if self.inner.fork_pending() == Some(id) {
            // The native fork happened, but its new thread id is beyond the
            // frame bound; Sage creates no Chat for it.
            self.inner.complete_fork(id).map(|_| true)
        } else {
            self.inner.oversized_child_response(id)
        };
        if native.map_err(map_adapter)? {
            self.native_rpc_reply = Some(crate::acp::goal::NativeRpcReply::TooLarge);
        }
        Ok(())
    }

    fn take_native_rpc_reply(&mut self) -> Option<crate::acp::goal::NativeRpcReply> {
        self.native_rpc_reply.take()
    }

    fn mutate_goal(
        &mut self,
        action: crate::acp::goal::GoalAction,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        self.native_rpc_reply = None;
        match action {
            crate::acp::goal::GoalAction::PauseExisting => self.request(CodexAdapter::goal_pause),
            crate::acp::goal::GoalAction::ClearExisting => self.request(CodexAdapter::goal_clear),
        }
    }

    fn capabilities(&self) -> Option<crate::acp::intent::CapabilitySnapshotV1> {
        self.inner.effective_capabilities().ok()
    }

    fn command(&mut self, command: AdapterCommand) -> Result<AdapterEgress, AdapterRuntimeError> {
        if self.poisoned {
            return Err(AdapterRuntimeError::Operation);
        }
        let method = command_method(&command);
        let result = match command {
            AdapterCommand::Initialize => self.request(CodexAdapter::initialize),
            AdapterCommand::ModelPage { cursor } => {
                if self.model_catalog_idle_only
                    || self.model_pages_seen >= crate::acp::limits::LIMITS.codex.model_catalog_pages
                {
                    return Err(AdapterRuntimeError::Operation);
                }
                let egress = self.request(|adapter, id| adapter.models(id, cursor.as_ref()))?;
                self.model_list_requested = true;
                self.next_model_cursor = None;
                Ok(egress)
            }
            AdapterCommand::NewSession { native_root, model } => {
                self.reset_session_model(model.clone());
                let config = self
                    .mcp
                    .codex_config()
                    .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
                self.request(|adapter, id| {
                    let root = adapter.project_root(native_root)?;
                    let request = adapter.start(id, &root, model.as_ref())?;
                    match config {
                        Some(config) => request.with_config(config).map_err(Into::into),
                        None => Ok(request),
                    }
                })
            }
            AdapterCommand::ResumeSession {
                external_session_id,
                native_root,
                model,
            } => {
                self.reset_session_model(model.clone());
                let config = self
                    .mcp
                    .codex_config()
                    .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
                self.request(|adapter, id| {
                    let root = adapter.project_root(native_root)?;
                    let request =
                        adapter.resume(id, &external_session_id, &root, model.as_ref())?;
                    match config {
                        Some(config) => request.with_config(config).map_err(Into::into),
                        None => Ok(request),
                    }
                })
            }
            AdapterCommand::LoadSession {
                external_session_id,
            } => {
                self.reset_session_model(None);
                self.request(|adapter, id| adapter.read(id, &external_session_id))
            }
            AdapterCommand::CloseSession => self.request(CodexAdapter::close),
            AdapterCommand::Prompt {
                prompt,
                client_message_id,
                access_mode,
            } => {
                let egress = self.prompt_request(&prompt, &client_message_id, access_mode)?;
                self.finish_model_config_hydration();
                Ok(egress)
            }
            AdapterCommand::Steer {
                prompt,
                client_message_id,
            } => self.request(|adapter, id| adapter.steer(id, &prompt, &client_message_id)),
            AdapterCommand::Cancel => self.request_with_control(CodexAdapter::cancel, true),
            AdapterCommand::ResolvePermission {
                request_id,
                decision,
            } => {
                let pending = self
                    .inner
                    .resolve_pending_permission(request_id.as_str(), &decision)
                    .map_err(|_| AdapterRuntimeError::LaterPermissionMiss)?;
                match pending {
                    Some((_, bytes, _)) if decision != PermissionDecision::Ask => {
                        let frame =
                            OutboundFrame::new(bytes).map_err(|_| AdapterRuntimeError::Frame)?;
                        resolve_later_permission(
                            &request_id,
                            &decision,
                            Some(AdapterEgress::new(
                                frame,
                                EgressToken::protocol_notification(),
                            )),
                        )
                        .map_err(|_| AdapterRuntimeError::LaterPermissionMiss)?
                        .ok_or(AdapterRuntimeError::LaterPermissionMiss)
                    }
                    _ => Err(AdapterRuntimeError::Operation),
                }
            }
        };
        if let Ok(egress) = result.as_ref() {
            tracing::info!(
                jsonrpc_method = method,
                jsonrpc_request_id = ?egress.token().request_id(),
                "Codex ACP outbound request queued"
            );
        }
        result
    }

    fn cancel_permission(
        &mut self,
        request_id: &crate::acp::contract::SageServerRequestId,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        let bytes = self
            .inner
            .cancelled_permission_response(request_id.as_str())
            .map_err(|_| AdapterRuntimeError::PermissionIntent)?;
        let frame = OutboundFrame::new(bytes).map_err(|_| AdapterRuntimeError::Frame)?;
        let external = ExternalId::new("permissionRequest", request_id.as_str())
            .map_err(|_| AdapterRuntimeError::PermissionIntent)?;
        Ok(AdapterEgress::new(
            frame,
            EgressToken::permission_cancel(external),
        ))
    }

    fn ingest(
        &mut self,
        frame: &Frame,
        resources: &dyn ResourceMapper,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        self.ingest_frame(frame, resources)
    }
    fn commit_mandatory_egress(&mut self, token: &EgressToken) -> Result<(), AdapterRuntimeError> {
        if let Some(request_id) = token.cancelled_permission_request() {
            return self
                .inner
                .advance_permission(request_id, ServerRequestState::Cancelled)
                .map_err(|_| AdapterRuntimeError::MandatoryDenyTeardown);
        }
        let Some((request_id, replay)) = token.deny_request() else {
            return Err(AdapterRuntimeError::EgressIntent);
        };
        if replay {
            return Ok(());
        }
        self.inner
            .advance_permission(request_id, ServerRequestState::DeclineCommitted)
            .map_err(|_| AdapterRuntimeError::MandatoryDenyTeardown)
    }

    fn resolve_egress(
        &mut self,
        token: EgressToken,
        resolution: EgressResolution,
    ) -> Result<(), AdapterRuntimeError> {
        if let Some(request_id) = token.request_id() {
            if resolution == EgressResolution::DeliveryUnknown {
                match self.inner.pending_kind(request_id) {
                    Some(RequestKind::GoalGet) => self.inner.goal_get_delivery_unknown(request_id),
                    Some(RequestKind::GoalPause | RequestKind::GoalClear) => {
                        self.inner.goal_mutation_delivery_unknown(request_id)
                    }
                    _ => {}
                }
            }
            if matches!(
                resolution,
                EgressResolution::QueueRejected | EgressResolution::WriteZeroBytes
            ) {
                let kind = self.inner.pending_kind(request_id);
                self.inner
                    .reject_before_write(request_id)
                    .map_err(map_adapter)?;
                if kind.is_some_and(RequestKind::starts_turn) {
                    self.snapshots.clear();
                }
                // The catalogue is read as far as it will be.
                if kind == Some(RequestKind::Models) {
                    self.refresh_model_config();
                }
            }
            return Ok(());
        }
        if token.is_notification() {
            return match resolution {
                EgressResolution::QueueRejected
                | EgressResolution::WriteZeroBytes
                | EgressResolution::DeliveryUnknown => Err(AdapterRuntimeError::EgressIntent),
                EgressResolution::Queued | EgressResolution::Written => Ok(()),
            };
        }
        if let Some(request_id) = token.allow_request() {
            let next = match resolution {
                EgressResolution::Queued => ServerRequestState::ResponseQueued,
                EgressResolution::Written => ServerRequestState::ResolvedAllowed,
                EgressResolution::QueueRejected
                | EgressResolution::WriteZeroBytes
                | EgressResolution::DeliveryUnknown => {
                    return Err(AdapterRuntimeError::MandatoryDenyTeardown);
                }
            };
            return self
                .inner
                .advance_permission(request_id, next)
                .map_err(|_| AdapterRuntimeError::MandatoryDenyTeardown);
        }
        if let Some(request_id) = token.cancelled_permission_request() {
            if matches!(
                resolution,
                EgressResolution::QueueRejected
                    | EgressResolution::WriteZeroBytes
                    | EgressResolution::DeliveryUnknown
            ) {
                return Err(AdapterRuntimeError::MandatoryDenyTeardown);
            }
            return self
                .inner
                .advance_permission(request_id, ServerRequestState::Cancelled)
                .map_err(|_| AdapterRuntimeError::MandatoryDenyTeardown);
        }
        let Some((request_id, replay)) = token.deny_request() else {
            return Err(AdapterRuntimeError::EgressIntent);
        };
        if matches!(
            resolution,
            EgressResolution::QueueRejected
                | EgressResolution::WriteZeroBytes
                | EgressResolution::DeliveryUnknown
        ) {
            return Err(AdapterRuntimeError::MandatoryDenyTeardown);
        }
        if replay {
            return Ok(());
        }
        let next = match resolution {
            EgressResolution::Queued => ServerRequestState::ResponseQueued,
            EgressResolution::Written => ServerRequestState::ResolvedDeclined,
            EgressResolution::QueueRejected
            | EgressResolution::WriteZeroBytes
            | EgressResolution::DeliveryUnknown => unreachable!(),
        };
        self.inner
            .advance_permission(request_id, next)
            .map_err(|_| AdapterRuntimeError::MandatoryDenyTeardown)
    }
}

mod browser_failure;
mod command_method;
use command_method::command_method;
