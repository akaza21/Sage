use crate::acp::{
    bindings::adapter::{
        AdapterActivityItem, AdapterBatch, AdapterCommand, AdapterEgress, AdapterEvent,
        AdapterIdentity, AdapterRuntimeError, EgressResolution, EgressToken, HarnessAdapter,
        ResourceMapper,
    },
    contract::{RequiredText, RuntimeEpoch, SafeText, SageAgentSessionId, SchemaVersionV1},
    intent::{
        ActivityProvenanceV1, ActivitySourceKind, CapabilitySnapshotV1, ExternalId,
        NormalizedPayload, ToolCategory,
    },
    lifecycle::{ActivityLifecycle, ItemLifecycle, ToolState},
    projection::ActivityRole,
    transport::{Frame, OutboundFrame},
};

pub(in crate::acp::fixture) struct ActivityFixtureAdapter {
    binding_digest: [u8; 32],
    runtime_epoch: RuntimeEpoch,
    session_id: SageAgentSessionId,
    client_message_id: RequiredText<512>,
    prompt_stage: PromptStage,
    parent_seen: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PromptStage {
    Idle,
    Created,
    Queued,
    Written,
}

impl ActivityFixtureAdapter {
    pub(in crate::acp::fixture) fn new(
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
            prompt_stage: PromptStage::Idle,
            parent_seen: false,
        }
    }

    fn identity(
        &self,
        field: &'static str,
        value: &str,
    ) -> Result<AdapterIdentity, AdapterRuntimeError> {
        Ok(AdapterIdentity {
            binding_digest: self.binding_digest,
            runtime_epoch: self.runtime_epoch,
            session_id: self.session_id.clone(),
            external_id: ExternalId::new(field, value).map_err(|_| AdapterRuntimeError::Frame)?,
            turn_id: Some(
                ExternalId::new("fixtureTurnId", "fixture-turn")
                    .map_err(|_| AdapterRuntimeError::Frame)?,
            ),
            turn_client_message_id: Some(self.client_message_id.clone()),
        })
    }

    fn activity(
        &self,
        activity: &str,
        parent: &str,
        text: &str,
    ) -> Result<AdapterEvent, AdapterRuntimeError> {
        Ok(AdapterEvent::ActivityItem(Box::new(AdapterActivityItem {
            activity_id: self.identity("externalActivityId", activity)?,
            item_id: self.identity("externalItemId", &format!("{activity}:text"))?,
            parent_activity_id: None,
            parent_item_id: self.identity("parentExternalItemId", parent)?,
            activity_kind: RequiredText::new("activityKind", "nativeChild")
                .map_err(|_| AdapterRuntimeError::Frame)?,
            role: ActivityRole::Child,
            activity_lifecycle: ActivityLifecycle::Observed,
            depth: 1,
            observed_sequence: 1,
            provenance: ActivityProvenanceV1 {
                schema_version: SchemaVersionV1,
                source_kind: ActivitySourceKind::Collaboration,
                external_kind: RequiredText::new("externalKind", "nativeChild")
                    .map_err(|_| AdapterRuntimeError::Frame)?,
                extension_schema_id: None,
                safe_label: Some(
                    RequiredText::new("activityLabel", "Native child activity")
                        .map_err(|_| AdapterRuntimeError::Frame)?,
                ),
            },
            item_lifecycle: ItemLifecycle::Completed,
            item_tool_state: None,
            payload: NormalizedPayload::Text {
                text: SafeText::new("activityText", text)
                    .map_err(|_| AdapterRuntimeError::Frame)?,
            },
        })))
    }
}

impl HarnessAdapter for ActivityFixtureAdapter {
    fn capabilities(&self) -> Option<CapabilitySnapshotV1> {
        None
    }

    fn command(&mut self, command: AdapterCommand) -> Result<AdapterEgress, AdapterRuntimeError> {
        let AdapterCommand::Prompt {
            client_message_id, ..
        } = command
        else {
            return Err(AdapterRuntimeError::UnsupportedCommand);
        };
        if self.prompt_stage != PromptStage::Idle || client_message_id != self.client_message_id {
            return Err(AdapterRuntimeError::Operation);
        }
        self.prompt_stage = PromptStage::Created;
        Ok(AdapterEgress::new(
            OutboundFrame::new(
                br#"{"jsonrpc":"2.0","method":"fixture/prompt","params":{}}"#.to_vec(),
            )
            .map_err(|_| AdapterRuntimeError::Frame)?,
            EgressToken::request(1),
        ))
    }

    fn ingest(
        &mut self,
        frame: &Frame,
        _resources: &dyn ResourceMapper,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        let envelope: serde_json::Value =
            serde_json::from_slice(frame.as_bytes()).map_err(|_| AdapterRuntimeError::Frame)?;
        if envelope.get("jsonrpc").and_then(serde_json::Value::as_str) != Some("2.0")
            || envelope.get("id").is_some()
        {
            return Err(AdapterRuntimeError::Frame);
        }
        let method = envelope
            .get("method")
            .and_then(serde_json::Value::as_str)
            .ok_or(AdapterRuntimeError::Frame)?;
        let params = envelope
            .get("params")
            .and_then(serde_json::Value::as_object)
            .ok_or(AdapterRuntimeError::Frame)?;
        let field = |name| {
            params
                .get(name)
                .and_then(serde_json::Value::as_str)
                .ok_or(AdapterRuntimeError::Frame)
        };
        let event = match method {
            "fixture/parentToolStart"
                if self.prompt_stage == PromptStage::Written && !self.parent_seen =>
            {
                self.parent_seen = true;
                AdapterEvent::Item {
                    id: self.identity("externalItemId", field("externalItemId")?)?,
                    lifecycle: ItemLifecycle::Streaming,
                    tool_state: Some(ToolState::InProgress),
                    payload: NormalizedPayload::ToolCall {
                        category: ToolCategory::Other,
                        label: SafeText::new("toolLabel", "Fixture parent tool")
                            .map_err(|_| AdapterRuntimeError::Frame)?,
                        input_summary: None,
                        output_summary: None,
                        locations: Default::default(),
                        command_activity: Default::default(),
                    },
                }
            }
            "fixture/childActivityText" if self.parent_seen => self.activity(
                field("externalActivityId")?,
                field("parentExternalItemId")?,
                field("text")?,
            )?,
            "fixture/childActivityForeignParent" => self.activity(
                field("externalActivityId")?,
                field("parentExternalItemId")?,
                "fixture foreign activity",
            )?,
            _ => return Err(AdapterRuntimeError::Operation),
        };
        AdapterBatch::new(vec![event], Vec::new())
    }

    fn commit_mandatory_egress(&mut self, _token: &EgressToken) -> Result<(), AdapterRuntimeError> {
        Err(AdapterRuntimeError::EgressIntent)
    }

    fn resolve_egress(
        &mut self,
        token: EgressToken,
        resolution: EgressResolution,
    ) -> Result<(), AdapterRuntimeError> {
        if token.request_id() != Some(1) {
            return Err(AdapterRuntimeError::EgressIntent);
        }
        match (self.prompt_stage, resolution) {
            (PromptStage::Created, EgressResolution::Queued) => {
                self.prompt_stage = PromptStage::Queued;
                Ok(())
            }
            (PromptStage::Queued, EgressResolution::Written) => {
                self.prompt_stage = PromptStage::Written;
                Ok(())
            }
            _ => Err(AdapterRuntimeError::EgressIntent),
        }
    }
}
