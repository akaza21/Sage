use crate::{
    acp::{
        bindings::adapter::{
            AdapterBatch, AdapterCommand, AdapterEgress, AdapterEvent, AdapterIdentity,
            AdapterRuntimeError, EgressResolution, EgressToken, HarnessAdapter, ResourceMapper,
        },
        contract::{RequiredText, RuntimeEpoch, SafeText, SageAgentSessionId},
        intent::{CapabilitySnapshotV1, ExternalId, NormalizedPayload, ToolCategory},
        semantic_ingress::{SemanticIngressError, validate_permission_payload},
        transport::{Frame, OutboundFrame, request_payload_digest},
    },
    acp_protocol::permissions::PermissionDecision,
};

pub(super) struct PermissionFixtureAdapter {
    binding_digest: [u8; 32],
    runtime_epoch: RuntimeEpoch,
    session_id: SageAgentSessionId,
    client_message_id: RequiredText<512>,
    first_request_digest: Option<[u8; 32]>,
    decline_stage: Option<DeclineStage>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DeclineStage {
    Prepared,
    Committed,
    Queued,
    Written,
}

impl PermissionFixtureAdapter {
    pub(super) fn new(
        binding_digest: [u8; 32],
        runtime_epoch: RuntimeEpoch,
        session_id: SageAgentSessionId,
        client_message_id: RequiredText<512>,
    ) -> Self {
        Self {
            binding_digest,
            runtime_epoch,
            session_id,
            client_message_id,
            first_request_digest: None,
            decline_stage: None,
        }
    }

    fn identity(&self, external_id: &str) -> Result<AdapterIdentity, AdapterRuntimeError> {
        Ok(AdapterIdentity {
            binding_digest: self.binding_digest,
            runtime_epoch: self.runtime_epoch,
            session_id: self.session_id.clone(),
            external_id: ExternalId::new("fixturePermissionId", external_id)
                .map_err(|_| AdapterRuntimeError::Frame)?,
            turn_id: Some(
                ExternalId::new("fixtureTurnId", "fixture-turn")
                    .map_err(|_| AdapterRuntimeError::Frame)?,
            ),
            turn_client_message_id: Some(self.client_message_id.clone()),
        })
    }

    fn ingest_permission(&mut self, frame: &Frame) -> Result<AdapterBatch, AdapterRuntimeError> {
        let envelope: serde_json::Value =
            serde_json::from_slice(frame.as_bytes()).map_err(|_| AdapterRuntimeError::Frame)?;
        let method = envelope
            .get("method")
            .and_then(serde_json::Value::as_str)
            .ok_or(AdapterRuntimeError::Frame)?;
        let payload = match method {
            "fixture/permissionRequestA" | "fixture/permissionRequestAConflictingPayload" => {
                frame.as_bytes()
            }
            "fixture/emitPermissionPayload" => envelope
                .pointer("/params/value")
                .and_then(serde_json::Value::as_str)
                .ok_or(AdapterRuntimeError::Frame)?
                .as_bytes(),
            _ => return Err(AdapterRuntimeError::Frame),
        };
        validate_permission_payload(payload).map_err(map_semantic)?;
        let request_digest =
            request_payload_digest(frame).map_err(|_| AdapterRuntimeError::Frame)?;
        if let Some(first) = self.first_request_digest {
            if first != request_digest {
                return Err(AdapterRuntimeError::MandatoryDenyTeardown);
            }
            if self.decline_stage != Some(DeclineStage::Written) {
                return Err(AdapterRuntimeError::MandatoryDenyTeardown);
            }
            return AdapterBatch::new(
                vec![AdapterEvent::IgnoredDuplicate],
                vec![decline_egress(true)?],
            )
            .map_err(|_| AdapterRuntimeError::MandatoryDenyTeardown);
        }
        self.first_request_digest = Some(request_digest);
        self.decline_stage = Some(DeclineStage::Prepared);
        let turn_id = self.identity("permission-turn-envelope")?;
        let item_id = self.identity("permission-a")?;
        let external_request_id = item_id.external_id.clone();
        AdapterBatch::new(
            vec![AdapterEvent::PermissionRequest {
                turn_id,
                item_id,
                external_request_id,
                payload: NormalizedPayload::PermissionRequest {
                    category: ToolCategory::Other,
                    summary: SafeText::new(
                        "permissionSummary",
                        "Fixture permission request; Sage policy requires denial",
                    )
                    .map_err(|_| AdapterRuntimeError::Frame)?,
                    offered_option_kinds: None,
                    reason: None,
                    user_input: None,
                },
                request_payload_digest: request_digest,
            }],
            Vec::new(),
        )
    }
}

impl HarnessAdapter for PermissionFixtureAdapter {
    fn capabilities(&self) -> Option<CapabilitySnapshotV1> {
        None
    }

    fn command(&mut self, command: AdapterCommand) -> Result<AdapterEgress, AdapterRuntimeError> {
        let AdapterCommand::ResolvePermission { decision, .. } = command else {
            return Err(AdapterRuntimeError::UnsupportedCommand);
        };
        if self.decline_stage != Some(DeclineStage::Prepared) {
            return Err(AdapterRuntimeError::PermissionIntent);
        }
        match decision {
            PermissionDecision::Ask => Ok(AdapterEgress::new(
                OutboundFrame::new(
                    br#"{"jsonrpc":"2.0","method":"sage/permissionAsked"}"#.to_vec(),
                )
                .map_err(|_| AdapterRuntimeError::Frame)?,
                EgressToken::protocol_notification(),
            )),
            PermissionDecision::AllowOnce => {
                let request_id = ExternalId::new("fixturePermissionId", "permission-a")
                    .map_err(|_| AdapterRuntimeError::Frame)?;
                Ok(AdapterEgress::new(
                    OutboundFrame::new(
                        br#"{"jsonrpc":"2.0","method":"fixture/permissionAllow","params":{"requestId":"permission-a"}}"#
                            .to_vec(),
                    )
                    .map_err(|_| AdapterRuntimeError::Frame)?,
                    EgressToken::permission_allow(request_id),
                ))
            }
            PermissionDecision::AllowAlways | PermissionDecision::Answer(_) => {
                Err(AdapterRuntimeError::PermissionIntent)
            }
            PermissionDecision::Deny => decline_egress(false),
        }
    }

    fn ingest(
        &mut self,
        frame: &Frame,
        _resources: &dyn ResourceMapper,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        self.ingest_permission(frame)
    }

    fn commit_mandatory_egress(&mut self, token: &EgressToken) -> Result<(), AdapterRuntimeError> {
        match (token.deny_request(), self.decline_stage) {
            (Some(("permission-a", false)), Some(DeclineStage::Prepared)) => {
                self.decline_stage = Some(DeclineStage::Committed);
                Ok(())
            }
            (Some(("permission-a", true)), Some(DeclineStage::Written)) => Ok(()),
            _ => Err(AdapterRuntimeError::EgressIntent),
        }
    }

    fn resolve_egress(
        &mut self,
        token: EgressToken,
        resolution: EgressResolution,
    ) -> Result<(), AdapterRuntimeError> {
        if token.allow_request().is_some() {
            return match (resolution, self.decline_stage) {
                (EgressResolution::Queued, Some(DeclineStage::Prepared)) => {
                    self.decline_stage = Some(DeclineStage::Queued);
                    Ok(())
                }
                (EgressResolution::Written, Some(DeclineStage::Queued)) => {
                    self.decline_stage = Some(DeclineStage::Written);
                    Ok(())
                }
                _ => Err(AdapterRuntimeError::PermissionIntent),
            };
        }
        match (token.deny_request(), resolution, self.decline_stage) {
            (
                Some(("permission-a", false)),
                EgressResolution::Queued,
                Some(DeclineStage::Committed),
            ) => {
                self.decline_stage = Some(DeclineStage::Queued);
                Ok(())
            }
            (
                Some(("permission-a", false)),
                EgressResolution::Written,
                Some(DeclineStage::Queued),
            ) => {
                self.decline_stage = Some(DeclineStage::Written);
                Ok(())
            }
            (
                Some(("permission-a", true)),
                EgressResolution::Queued | EgressResolution::Written,
                Some(DeclineStage::Written),
            ) => Ok(()),
            (Some(("permission-a", _)), _, _) => Err(AdapterRuntimeError::MandatoryDenyTeardown),
            _ => Err(AdapterRuntimeError::EgressIntent),
        }
    }
}

fn decline_egress(replay: bool) -> Result<AdapterEgress, AdapterRuntimeError> {
    let request_id = ExternalId::new("fixturePermissionId", "permission-a")
        .map_err(|_| AdapterRuntimeError::Frame)?;
    Ok(AdapterEgress::new(
        OutboundFrame::new(
            br#"{"jsonrpc":"2.0","method":"fixture/permissionDecline","params":{"requestId":"permission-a","decision":"declined"}}"#
                .to_vec(),
        )
        .map_err(|_| AdapterRuntimeError::Frame)?,
        EgressToken::mandatory_deny(request_id, replay),
    ))
}

fn map_semantic(error: SemanticIngressError) -> AdapterRuntimeError {
    match error {
        SemanticIngressError::Limit => AdapterRuntimeError::BatchBytes,
        SemanticIngressError::Invalid => AdapterRuntimeError::Frame,
    }
}
