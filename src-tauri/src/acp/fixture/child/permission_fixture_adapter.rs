use crate::acp::{
    bindings::adapter::{
        AdapterBatch, AdapterCommand, AdapterEgress, AdapterEvent, AdapterIdentity,
        AdapterRuntimeError, EgressResolution, EgressToken, HarnessAdapter, ResourceMapper,
    },
    contract::{RequiredText, RuntimeEpoch, SafeText, SageAgentSessionId},
    intent::{CapabilitySnapshotV1, ExternalId, NormalizedPayload, ToolCategory},
    semantic_ingress::{SemanticIngressError, validate_permission_payload},
    transport::{Frame, OutboundFrame, request_payload_digest},
};

pub(super) struct PermissionFixtureAdapter {
    binding_digest: [u8; 32],
    runtime_epoch: RuntimeEpoch,
    session_id: SageAgentSessionId,
    client_message_id: RequiredText<512>,
    first_request_digest: Option<[u8; 32]>,
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
            return Err(AdapterRuntimeError::Operation);
        }
        self.first_request_digest = Some(request_digest);
        let turn_id = self.identity("permission-turn-envelope")?;
        let item_id = self.identity("permission-a")?;
        AdapterBatch::new(
            vec![AdapterEvent::PermissionRequest {
                turn_id,
                item_id,
                payload: NormalizedPayload::PermissionRequest {
                    category: ToolCategory::Other,
                    summary: SafeText::new(
                        "permissionSummary",
                        "Fixture permission request; Sage policy requires denial",
                    )
                    .map_err(|_| AdapterRuntimeError::Frame)?,
                },
                request_payload_digest: request_digest,
            }],
            vec![decline_egress()?],
        )
    }
}

impl HarnessAdapter for PermissionFixtureAdapter {
    fn capabilities(&self) -> Option<CapabilitySnapshotV1> {
        None
    }

    fn command(&mut self, _command: AdapterCommand) -> Result<AdapterEgress, AdapterRuntimeError> {
        Err(AdapterRuntimeError::UnsupportedCommand)
    }

    fn ingest(
        &mut self,
        frame: &Frame,
        _resources: &dyn ResourceMapper,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        self.ingest_permission(frame)
    }

    fn commit_mandatory_egress(&mut self, token: &EgressToken) -> Result<(), AdapterRuntimeError> {
        match token.deny_request() {
            Some(("permission-a", _)) if self.first_request_digest.is_some() => Ok(()),
            _ => Err(AdapterRuntimeError::EgressIntent),
        }
    }

    fn resolve_egress(
        &mut self,
        token: EgressToken,
        _resolution: EgressResolution,
    ) -> Result<(), AdapterRuntimeError> {
        match token.deny_request() {
            Some(("permission-a", _)) => Ok(()),
            _ => Err(AdapterRuntimeError::EgressIntent),
        }
    }
}

fn decline_egress() -> Result<AdapterEgress, AdapterRuntimeError> {
    let request_id = ExternalId::new("fixturePermissionId", "permission-a")
        .map_err(|_| AdapterRuntimeError::Frame)?;
    Ok(AdapterEgress::new(
        OutboundFrame::new(
            br#"{"jsonrpc":"2.0","method":"fixture/permissionDecline","params":{"requestId":"permission-a","decision":"declined"}}"#
                .to_vec(),
        )
        .map_err(|_| AdapterRuntimeError::Frame)?,
        EgressToken::mandatory_deny(request_id, false),
    ))
}

fn map_semantic(error: SemanticIngressError) -> AdapterRuntimeError {
    match error {
        SemanticIngressError::Limit => AdapterRuntimeError::BatchBytes,
        SemanticIngressError::Invalid => AdapterRuntimeError::Frame,
    }
}
