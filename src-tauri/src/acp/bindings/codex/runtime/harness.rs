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
            AdapterCommand::NewSession { native_root, model } => self.request(|adapter, id| {
                let root = adapter.project_root(native_root)?;
                adapter.start(id, &root, model.as_ref())
            }),
            AdapterCommand::ResumeSession {
                external_session_id,
                native_root,
                model,
            } => self.request(|adapter, id| {
                let root = adapter.project_root(native_root)?;
                adapter.resume(id, &external_session_id, &root, model.as_ref())
            }),
            AdapterCommand::LoadSession {
                external_session_id,
            } => self.request(|adapter, id| adapter.read(id, &external_session_id)),
            AdapterCommand::CloseSession => self.request(CodexAdapter::close),
            AdapterCommand::Prompt {
                prompt,
                client_message_id,
            } => self.request(|adapter, id| adapter.prompt(id, &prompt, &client_message_id)),
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
                    (PermissionDecision::AllowOnce, Some((_, bytes, _))) => {
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
        let mapper = MapperBridge(resources);
        let disposition = self
            .inner
            .ingest_with_declarations(frame.as_bytes(), &mapper, &self.declared_test_commands)
            .map_err(map_adapter)?;
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

fn command_method(command: &AdapterCommand) -> &'static str {
    match command {
        AdapterCommand::Initialize => "initialize",
        AdapterCommand::ModelPage { .. } => "model/list",
        AdapterCommand::NewSession { .. } => "thread/start",
        AdapterCommand::ResumeSession { .. } => "thread/resume",
        AdapterCommand::LoadSession { .. } => "thread/read",
        AdapterCommand::CloseSession => "thread/archive",
        AdapterCommand::Prompt { .. } => "turn/start",
        AdapterCommand::Cancel => "turn/interrupt",
        AdapterCommand::ResolvePermission { .. } => "requestPermission",
    }
}
