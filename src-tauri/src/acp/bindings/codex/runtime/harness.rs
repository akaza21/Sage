use super::*;
use crate::acp::{
    bindings::adapter::{
        AdapterBatch, AdapterCommand, AdapterEgress, AdapterRuntimeError, EgressResolution,
        EgressToken, HarnessAdapter, ResourceMapper,
    },
    intent::ExternalId,
    lifecycle::ServerRequestState,
    runtime::{later_permission::resolve_later_permission, permissions::PermissionDecision},
    transport::{Frame, OutboundFrame, request_payload_digest},
};

use super::super::{request::RequestKind, state::CodexAdapter};

impl HarnessAdapter for CodexRuntime {
    fn provisional_turn_pending(&self) -> bool {
        self.inner.has_provisional_turn()
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
        self.inner.goal_mutation_pending() || self.inner.child_read_pending()
    }

    fn child_read_pending(&self) -> bool {
        self.inner.child_read_pending()
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
        if self
            .inner
            .oversized_child_response(id)
            .map_err(map_adapter)?
        {
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
                self.request(|adapter, id| adapter.models(id, cursor.as_ref()))
            }
            AdapterCommand::NewSession { native_root, model } => {
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
            } => self.request(|adapter, id| adapter.read(id, &external_session_id)),
            AdapterCommand::CloseSession => self.request(CodexAdapter::close),
            AdapterCommand::Prompt {
                prompt,
                client_message_id,
                access_mode,
            } => self.request(|adapter, id| {
                adapter.prompt_with_access_mode(id, &prompt, &client_message_id, access_mode)
            }),
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
                    .resolve_pending_permission(request_id.as_str(), decision)
                    .map_err(|_| AdapterRuntimeError::LaterPermissionMiss)?;
                match (decision, pending) {
                    (PermissionDecision::Ask, None) => Err(AdapterRuntimeError::Operation),
                    (
                        PermissionDecision::AllowOnce | PermissionDecision::AllowAlways,
                        Some((_, bytes, _)),
                    ) => {
                        let frame =
                            OutboundFrame::new(bytes).map_err(|_| AdapterRuntimeError::Frame)?;
                        resolve_later_permission(
                            &request_id,
                            decision,
                            Some(AdapterEgress::new(
                                frame,
                                EgressToken::protocol_notification(),
                            )),
                        )
                        .map_err(|_| AdapterRuntimeError::LaterPermissionMiss)?
                        .ok_or(AdapterRuntimeError::LaterPermissionMiss)
                    }
                    (PermissionDecision::Deny, Some((_, bytes, _))) => {
                        let frame =
                            OutboundFrame::new(bytes).map_err(|_| AdapterRuntimeError::Frame)?;
                        resolve_later_permission(
                            &request_id,
                            decision,
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
        if self.poisoned {
            return Err(AdapterRuntimeError::Operation);
        }
        if self.inner.child_read_pending() || self.inner.is_child_read_tombstone_id_active() {
            let value: serde_json::Value =
                serde_json::from_slice(frame.as_bytes()).map_err(|_| AdapterRuntimeError::Frame)?;
            if let Some(id) = value.get("id").and_then(serde_json::Value::as_u64) {
                if self.inner.is_child_read_tombstone(id)
                    && value.get("method").is_none()
                    && (value.get("result").is_some() || value.get("error").is_some())
                {
                    return one(AdapterEvent::IgnoredLate);
                }
                if let Some((pending, child, parent)) = self.inner.child_read_scope()
                    && id == pending
                {
                    let snapshot = super::super::child_transcript::normalize_read_response(
                        frame.as_bytes(),
                        id,
                        child,
                        parent,
                        resources,
                    )
                    .map_err(|_| AdapterRuntimeError::Frame)?;
                    self.inner.complete_child_read(id).map_err(map_adapter)?;
                    self.native_rpc_reply =
                        Some(crate::acp::goal::NativeRpcReply::ChildTranscript(snapshot));
                    return one(AdapterEvent::IgnoredLate);
                }
            }
        }
        let mapper = MapperBridge(resources);
        let disposition = self
            .inner
            .ingest_with_declarations(frame.as_bytes(), &mapper, &self.declared_test_commands)
            .map_err(map_adapter)?;
        self.record_browser_failure_diagnostic(&disposition, frame.as_bytes());
        let request_digest =
            request_payload_digest(frame).map_err(|_| AdapterRuntimeError::Frame)?;
        self.translate(disposition, request_digest)
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
                let clear = self.inner.pending_kind(request_id) == Some(RequestKind::TurnStart);
                self.inner
                    .reject_before_write(request_id)
                    .map_err(map_adapter)?;
                if clear {
                    self.snapshots.clear();
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

impl CodexRuntime {
    fn record_browser_failure_diagnostic(
        &mut self,
        disposition: &IngressDisposition,
        frame: &[u8],
    ) {
        if self.browser_failure_warning_logged {
            return;
        }
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(frame) else {
            return;
        };
        let failure = (value["method"] == "mcpServer/startupStatus/updated"
            && value["params"]["name"] == "sage-browser"
            && value["params"]["status"] == "failed")
            .then(|| {
                let thread = value["params"]["threadId"].as_str()?;
                let error = value["params"]["error"].as_str()?;
                Some((thread, self.mcp.browser_failure_detail(error), error.len()))
            })
            .flatten();
        if let Some((thread, detail, bytes)) = failure.as_ref()
            && !self.inner.has_active_turn()
            && !self
                .pending_browser_failure_details
                .iter()
                .any(|(id, _, _)| id == thread)
        {
            if self.pending_browser_failure_details.len() == 8 {
                self.pending_browser_failure_details.remove(0);
            }
            self.pending_browser_failure_details.push((
                (*thread).to_owned(),
                detail.clone(),
                *bytes,
            ));
        }
        let projected = matches!(disposition, IngressDisposition::Events(events) if events.iter().any(|event| {
            matches!(event, IncomingEvent::Error(crate::acp::intent::NormalizedPayload::Error { code, .. }) if code.as_str() == "browserMcpUnavailable")
        }));
        if !projected {
            return;
        }
        let thread = failure.as_ref().map(|(thread, _, _)| *thread).or_else(|| {
            value["result"]["thread"]["id"]
                .as_str()
                .or_else(|| value["params"]["thread"]["id"].as_str())
                .or_else(|| value["params"]["threadId"].as_str())
        });
        let pending = thread.and_then(|thread| {
            self.pending_browser_failure_details
                .iter()
                .position(|(id, _, _)| id == thread)
                .map(|index| self.pending_browser_failure_details.swap_remove(index))
        });
        let (detail, error_bytes) = failure
            .map(|(_, detail, bytes)| (detail, bytes))
            .or_else(|| pending.map(|(_, detail, bytes)| (detail, bytes)))
            .unwrap_or_else(|| ("browser failure detail unavailable".to_owned(), 0));
        tracing::warn!(
            detail = %detail,
            error_bytes,
            "Codex browser MCP startup failed"
        );
        self.browser_failure_warning_logged = true;
        self.pending_browser_failure_details.clear();
    }
}

fn command_method(command: &AdapterCommand) -> &'static str {
    match command {
        AdapterCommand::Initialize => "initialize",
        AdapterCommand::ModelPage { .. } => "model/list",
        AdapterCommand::NewSession { .. } => "thread/start",
        AdapterCommand::ResumeSession { .. } => "thread/resume",
        AdapterCommand::LoadSession { .. } => "thread/read",
        AdapterCommand::CloseSession => "thread/archive",
        AdapterCommand::Prompt { .. } => "turn/start",
        AdapterCommand::Steer { .. } => "turn/steer",
        AdapterCommand::Cancel => "turn/interrupt",
        AdapterCommand::ResolvePermission { .. } => "requestPermission",
    }
}
