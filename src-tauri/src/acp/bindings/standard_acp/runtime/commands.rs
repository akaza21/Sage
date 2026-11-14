use crate::acp::{
    bindings::adapter::{
        AdapterBatch, AdapterCommand, AdapterEgress, AdapterRuntimeError, EgressResolution,
        EgressToken, HarnessAdapter, ResourceMapper as SharedResourceMapper,
    },
    intent::CapabilitySnapshotV1,
    transport::{Frame, request_payload_digest},
};
use sha2::{Digest, Sha256};

use super::super::wire::decode_frame;
use super::{
    PendingRequest, PermissionReply, PermissionState, StandardAcpRuntime,
    ingress::permission_egress, map_translation,
};

pub(super) fn response_id(value: &serde_json::Value) -> Result<u64, AdapterRuntimeError> {
    value
        .get("id")
        .and_then(serde_json::Value::as_u64)
        .filter(|id| *id <= i64::MAX as u64)
        .ok_or(AdapterRuntimeError::Frame)
}

pub(crate) fn scoped_permission_id(
    session: &crate::acp::contract::SageAgentSessionId,
    epoch: crate::acp::contract::RuntimeEpoch,
    wire: &str,
) -> Result<crate::acp::intent::ExternalId, AdapterRuntimeError> {
    let session_digest = Sha256::digest(session.as_str().as_bytes());
    let wire_digest = Sha256::digest(wire.as_bytes());
    crate::acp::intent::ExternalId::new(
        "permissionIntentId",
        format!(
            "standard-acp:{session_digest:x}:{}:{wire_digest:x}",
            epoch.get()
        ),
    )
    .map_err(|_| AdapterRuntimeError::Frame)
}

impl HarnessAdapter for StandardAcpRuntime {
    fn receive_command_update(&mut self, frame: &Frame) -> Result<(), AdapterRuntimeError> {
        let value =
            serde_json::from_slice(frame.as_bytes()).map_err(|_| AdapterRuntimeError::Frame)?;
        let active = self.active.as_mut().ok_or(AdapterRuntimeError::Frame)?;
        super::super::surface::update_commands(
            &value,
            active.external_id.as_str(),
            &mut active.commands,
        )
        .map(|_| ())
    }
    fn available_commands(&self) -> std::sync::Arc<crate::acp::slash_commands::CommandState> {
        self.active
            .as_ref()
            .map_or_else(std::sync::Arc::default, |a| {
                std::sync::Arc::clone(&a.commands)
            })
    }
    fn capabilities(&self) -> Option<CapabilitySnapshotV1> {
        self.capabilities.clone()
    }

    fn refusal_detail(&self) -> Option<crate::acp::bindings::adapter::RefusalDetail> {
        self.refusal_detail.clone()
    }

    fn cancel_permission(
        &mut self,
        request_id: &crate::acp::contract::SageServerRequestId,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        self.cancel_elicitation(request_id.as_str())
            .unwrap_or(Err(AdapterRuntimeError::UnsupportedCommand))
    }

    fn command(&mut self, command: AdapterCommand) -> Result<AdapterEgress, AdapterRuntimeError> {
        match command {
            AdapterCommand::Steer { .. } => Err(AdapterRuntimeError::UnsupportedCommand),
            AdapterCommand::Initialize => self.initialize(),
            AdapterCommand::NewSession {
                native_root,
                model: None,
            } => self.new_session(native_root),
            AdapterCommand::Prompt {
                prompt,
                client_message_id,
                access_mode,
            } => self.prompt_with_mode(prompt, client_message_id, access_mode),
            AdapterCommand::Cancel => self.cancel(),
            AdapterCommand::CloseSession => self.close_session(),
            AdapterCommand::ResolvePermission {
                request_id,
                decision,
            } => {
                if let Some(result) = self.resolve_elicitation(request_id.as_str(), &decision) {
                    return result;
                }
                let record = self
                    .permissions
                    .get_mut(request_id.as_str())
                    .filter(|record| {
                        record.reply == PermissionReply::AwaitUser
                            && record.state == PermissionState::Pending
                    })
                    .ok_or(AdapterRuntimeError::LaterPermissionMiss)?;
                let reply = match decision {
                    crate::acp_protocol::permissions::PermissionDecision::AllowOnce => {
                        if record.allow_once_option_id.is_none() {
                            return Err(AdapterRuntimeError::UnsupportedCommand);
                        }
                        PermissionReply::AllowOnce
                    }
                    crate::acp_protocol::permissions::PermissionDecision::AllowAlways => {
                        if record.allow_always_option_id.is_none() {
                            return Err(AdapterRuntimeError::UnsupportedCommand);
                        }
                        PermissionReply::AllowAlways
                    }
                    crate::acp_protocol::permissions::PermissionDecision::Deny => {
                        PermissionReply::Deny
                    }
                    crate::acp_protocol::permissions::PermissionDecision::Ask => {
                        return Err(AdapterRuntimeError::Operation);
                    }
                    crate::acp_protocol::permissions::PermissionDecision::Answer(_) => {
                        return Err(AdapterRuntimeError::UnsupportedCommand);
                    }
                };
                let external =
                    crate::acp::intent::ExternalId::new("permissionRequest", request_id.as_str())
                        .map_err(|_| AdapterRuntimeError::LaterPermissionMiss)?;
                let egress = permission_egress(
                    &external,
                    &record.wire_id,
                    reply,
                    false,
                    match reply {
                        PermissionReply::AllowAlways => record.allow_always_option_id.as_deref(),
                        _ => record.allow_once_option_id.as_deref(),
                    },
                )?;
                record.reply = reply;
                Ok(egress)
            }
            AdapterCommand::ModelPage { .. }
            | AdapterCommand::ResumeSession { .. }
            | AdapterCommand::LoadSession { .. }
            | AdapterCommand::NewSession { model: Some(_), .. } => {
                Err(AdapterRuntimeError::UnsupportedCommand)
            }
        }
    }

    fn ingest(
        &mut self,
        frame: &Frame,
        resources: &dyn SharedResourceMapper,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        let value = decode_frame(frame.as_bytes()).map_err(|_| AdapterRuntimeError::Frame)?;
        if value.get("method").is_some() {
            let digest = value
                .get("id")
                .is_some()
                .then(|| request_payload_digest(frame).map_err(|_| AdapterRuntimeError::Frame))
                .transpose()?;
            self.ingest_notification(value, resources, digest)
        } else {
            self.ingest_response(value)
        }
    }

    fn commit_mandatory_egress(&mut self, token: &EgressToken) -> Result<(), AdapterRuntimeError> {
        if let Some(request_id) = token.cancelled_permission_request() {
            return self
                .settle_elicitation(request_id, None)
                .unwrap_or(Err(AdapterRuntimeError::MandatoryDenyTeardown));
        }
        let (request_id, replay) = token
            .deny_request()
            .ok_or(AdapterRuntimeError::EgressIntent)?;
        if !replay && let Some(result) = self.settle_elicitation(request_id, None) {
            return result;
        }
        let record = self
            .permissions
            .get_mut(request_id)
            .filter(|record| record.reply == PermissionReply::Deny)
            .ok_or(AdapterRuntimeError::MandatoryDenyTeardown)?;
        if replay {
            return (record.state == PermissionState::Written)
                .then_some(())
                .ok_or(AdapterRuntimeError::MandatoryDenyTeardown);
        }
        if record.state == PermissionState::Pending {
            let active = self
                .active
                .as_mut()
                .ok_or(AdapterRuntimeError::MandatoryDenyTeardown)?;
            let event = active
                .translator
                .permission_decline_committed(&record.translator_id)
                .map_err(|_| AdapterRuntimeError::MandatoryDenyTeardown)?;
            self.deferred.push(event);
            record.state = PermissionState::Committed;
        }
        Ok(())
    }

    fn resolve_egress(
        &mut self,
        token: EgressToken,
        resolution: EgressResolution,
    ) -> Result<(), AdapterRuntimeError> {
        if token.is_client_fs_response() {
            return matches!(
                resolution,
                EgressResolution::Queued | EgressResolution::Written
            )
            .then_some(())
            .ok_or(AdapterRuntimeError::MandatoryDenyTeardown);
        }
        if let Some(request_id) = token
            .deny_request()
            .filter(|(_, replay)| !replay)
            .map(|(id, _)| id)
            .or_else(|| token.allow_request())
            .or_else(|| token.cancelled_permission_request())
            && let Some(result) = self.settle_elicitation(request_id, Some(resolution))
        {
            return result;
        }
        if let Some((request_id, replay)) = token.deny_request() {
            let record = self
                .permissions
                .get_mut(request_id)
                .filter(|record| record.reply == PermissionReply::Deny)
                .ok_or(AdapterRuntimeError::MandatoryDenyTeardown)?;
            if replay {
                return (record.state == PermissionState::Written
                    && resolution == EgressResolution::Written)
                    .then_some(())
                    .ok_or(AdapterRuntimeError::MandatoryDenyTeardown);
            }
            record.state = match (record.state, resolution) {
                (PermissionState::Committed, EgressResolution::Queued)
                | (PermissionState::Queued, EgressResolution::Queued) => PermissionState::Queued,
                (PermissionState::Queued, EgressResolution::Written)
                | (PermissionState::Committed, EgressResolution::Written) => {
                    PermissionState::Written
                }
                _ => return Err(AdapterRuntimeError::MandatoryDenyTeardown),
            };
            return Ok(());
        }
        if let Some(request_id) = token.allow_request() {
            let record = self
                .permissions
                .get_mut(request_id)
                .filter(|record| {
                    matches!(
                        record.reply,
                        PermissionReply::AllowOnce | PermissionReply::AllowAlways
                    )
                })
                .ok_or(AdapterRuntimeError::EgressIntent)?;
            record.state = match (record.state, resolution) {
                (PermissionState::Pending, EgressResolution::Queued)
                | (PermissionState::Queued, EgressResolution::Queued) => PermissionState::Queued,
                (PermissionState::Queued, EgressResolution::Written)
                | (PermissionState::Pending, EgressResolution::Written) => PermissionState::Written,
                (PermissionState::Written, EgressResolution::Queued)
                | (PermissionState::Written, EgressResolution::Written) => PermissionState::Written,
                _ => return Err(AdapterRuntimeError::EgressIntent),
            };
            return Ok(());
        }
        if token.is_notification() {
            if resolution == EgressResolution::DeliveryUnknown {
                self.delivery_unknown = Some(0);
            }
            return Ok(());
        }
        let id = token
            .request_id()
            .ok_or(AdapterRuntimeError::EgressIntent)?;
        if !self.pending.contains_key(&id) || self.delivery_unknown.is_some() {
            return Err(AdapterRuntimeError::EgressIntent);
        }
        if matches!(
            resolution,
            EgressResolution::QueueRejected | EgressResolution::WriteZeroBytes
        ) {
            let pending = self
                .pending
                .remove(&id)
                .ok_or(AdapterRuntimeError::EgressIntent)?;
            match pending {
                PendingRequest::Prompt => self.rollback_prompt(id)?,
                PendingRequest::SetMode => self.pending_prompt = None,
                PendingRequest::CloseSession => self.close_pending = false,
                _ => {}
            }
        } else if resolution == EgressResolution::DeliveryUnknown {
            self.delivery_unknown = Some(id);
        }
        Ok(())
    }
}

impl StandardAcpRuntime {
    fn rollback_prompt(&mut self, request_id: u64) -> Result<(), AdapterRuntimeError> {
        let active = self
            .active
            .as_mut()
            .ok_or(AdapterRuntimeError::EgressIntent)?;
        if active.turn.as_ref().map(|turn| turn.request_id) != Some(request_id) {
            return Err(AdapterRuntimeError::EgressIntent);
        }
        active
            .translator
            .reject_prompt(&request_id.to_string())
            .map_err(map_translation)?;
        active.turn = None;
        Ok(())
    }
}
