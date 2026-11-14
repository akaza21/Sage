use serde::Deserialize;

mod activation;
mod cancel;

use crate::acp::{
    bindings::adapter::{
        AdapterBatch, AdapterCommand, AdapterEgress, AdapterEvent, AdapterIdentity,
        AdapterRuntimeError, EgressResolution, EgressToken, HarnessAdapter, ResourceMapper,
    },
    contract::{RequiredText, RuntimeEpoch, SafeText, SageAgentSessionId},
    intent::{CapabilitySnapshotV1, ExternalId, NormalizedPayload},
    lifecycle::{ItemLifecycle, ToolState, TurnState},
    transport::{Frame, OutboundFrame},
};
use sage_core::limits::LIMITS;

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Idle,
    InitializeCreated,
    InitializeQueued,
    AwaitingInitialize,
    Initialized,
    NewCreated,
    NewQueued,
    AwaitingNew,
    ResumeCreated,
    ResumeQueued,
    AwaitingResume,
    PromptCreated,
    PromptQueued,
    CancelCreated,
    CancelQueued,
    AwaitingTextStart,
    Streaming,
    ItemComplete,
    ToolComplete,
    TextComplete,
    Complete,
}

pub(crate) struct FixtureAdapter {
    binding_digest: [u8; 32],
    runtime_epoch: RuntimeEpoch,
    session_id: SageAgentSessionId,
    external_session_id: ExternalId,
    client_message_id: Option<RequiredText<512>>,
    capabilities: Option<CapabilitySnapshotV1>,
    next_request_id: u64,
    pending_request_id: Option<u64>,
    cancel_restore_state: Option<State>,
    session_ready: bool,
    state: State,
}

impl FixtureAdapter {
    pub(crate) fn new(
        binding_digest: [u8; 32],
        runtime_epoch: RuntimeEpoch,
        session_id: SageAgentSessionId,
        external_session_id: ExternalId,
    ) -> Result<Self, AdapterRuntimeError> {
        if binding_digest == [0; 32] || runtime_epoch == RuntimeEpoch::ZERO {
            return Err(AdapterRuntimeError::Operation);
        }
        Ok(Self {
            binding_digest,
            runtime_epoch,
            session_id,
            external_session_id,
            client_message_id: None,
            capabilities: None,
            next_request_id: 1,
            pending_request_id: None,
            cancel_restore_state: None,
            session_ready: false,
            state: State::Idle,
        })
    }

    fn request_with(
        &mut self,
        payload: Vec<u8>,
        token: impl FnOnce(u64) -> EgressToken,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        let id = self.next_request_id;
        self.next_request_id = id.checked_add(1).ok_or(AdapterRuntimeError::Operation)?;
        let frame = OutboundFrame::new(payload).map_err(|_| AdapterRuntimeError::Frame)?;
        self.pending_request_id = Some(id);
        Ok(AdapterEgress::new(frame, token(id)))
    }

    fn request(&mut self, payload: Vec<u8>) -> Result<AdapterEgress, AdapterRuntimeError> {
        self.request_with(payload, EgressToken::request)
    }

    fn session_identity(&self) -> AdapterIdentity {
        AdapterIdentity {
            binding_digest: self.binding_digest,
            runtime_epoch: self.runtime_epoch,
            session_id: self.session_id.clone(),
            external_id: self.external_session_id.clone(),
            turn_id: None,
            turn_client_message_id: None,
        }
    }

    fn identity(&self, external_id: &str) -> Result<AdapterIdentity, AdapterRuntimeError> {
        Ok(AdapterIdentity {
            binding_digest: self.binding_digest,
            runtime_epoch: self.runtime_epoch,
            session_id: self.session_id.clone(),
            external_id: ExternalId::new("fixtureExternalId", external_id)
                .map_err(|_| AdapterRuntimeError::Frame)?,
            turn_id: Some(
                ExternalId::new("fixtureTurnId", "fixture-turn-1")
                    .map_err(|_| AdapterRuntimeError::Frame)?,
            ),
            turn_client_message_id: Some(
                self.client_message_id
                    .clone()
                    .ok_or(AdapterRuntimeError::Operation)?,
            ),
        })
    }

    fn item(
        &self,
        lifecycle: ItemLifecycle,
        text: &str,
    ) -> Result<AdapterEvent, AdapterRuntimeError> {
        Ok(AdapterEvent::Item {
            id: self.identity("fixture-text-1")?,
            lifecycle,
            tool_state: None,
            payload: NormalizedPayload::Text {
                text: SafeText::new("fixtureText", text).map_err(|_| AdapterRuntimeError::Frame)?,
            },
        })
    }

    fn tool(&self, state: ToolState) -> Result<AdapterEvent, AdapterRuntimeError> {
        Ok(AdapterEvent::Item {
            id: self.identity("fixture-tool-1")?,
            lifecycle: match state {
                ToolState::Completed
                | ToolState::Failed
                | ToolState::Declined
                | ToolState::Cancelled => ItemLifecycle::Completed,
                _ => ItemLifecycle::Streaming,
            },
            tool_state: Some(state),
            payload: NormalizedPayload::ToolCall {
                category: crate::acp::intent::ToolCategory::Other,
                label: SafeText::new("toolLabel", "Fixture tool")
                    .map_err(|_| AdapterRuntimeError::Frame)?,
                input_summary: None,
                output_summary: None,
                locations: Default::default(),
                command_activity: Default::default(),
                execution: Default::default(),
            },
        })
    }
}

impl HarnessAdapter for FixtureAdapter {
    fn capabilities(&self) -> Option<CapabilitySnapshotV1> {
        self.capabilities.clone()
    }

    fn command(&mut self, command: AdapterCommand) -> Result<AdapterEgress, AdapterRuntimeError> {
        match command {
            command @ (AdapterCommand::Initialize
            | AdapterCommand::NewSession { .. }
            | AdapterCommand::ResumeSession { .. }) => self.activation_command(command),
            AdapterCommand::Prompt {
                client_message_id, ..
            } if self.state == State::Idle => {
                self.client_message_id = Some(client_message_id);
                let request = self.request(
                    br#"{"jsonrpc":"2.0","method":"fixture/prompt","params":{}}"#.to_vec(),
                )?;
                self.state = State::PromptCreated;
                Ok(request)
            }
            AdapterCommand::Prompt { .. } => Err(AdapterRuntimeError::Operation),
            AdapterCommand::Cancel => self.cancel_command(),
            _ => Err(AdapterRuntimeError::UnsupportedCommand),
        }
    }

    fn ingest(
        &mut self,
        frame: &Frame,
        _resources: &dyn ResourceMapper,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        let value: serde_json::Value =
            serde_json::from_slice(frame.as_bytes()).map_err(|_| AdapterRuntimeError::Frame)?;
        if value.get("schemaVersion").is_some() {
            if self.state != State::Idle || self.session_ready {
                return Err(AdapterRuntimeError::Operation);
            }
            if frame.len() > LIMITS.wire.capability_metadata_bytes as usize {
                return Err(AdapterRuntimeError::BatchBytes);
            }
            let capabilities = serde_json::from_value::<CapabilitySnapshotV1>(value)
                .map_err(|_| AdapterRuntimeError::Frame)?;
            self.capabilities = Some(capabilities.clone());
            return AdapterBatch::new(vec![AdapterEvent::Initialized { capabilities }], Vec::new());
        }
        if value.get("method").and_then(serde_json::Value::as_str) == Some("fixture/metadata") {
            return ingest_metadata(value);
        }
        let id_present = value
            .as_object()
            .is_some_and(|members| members.contains_key("id"));
        let envelope: Envelope =
            serde_json::from_value(value).map_err(|_| AdapterRuntimeError::Frame)?;
        if envelope.jsonrpc != "2.0" {
            return Err(AdapterRuntimeError::Frame);
        }
        if let Some(result) = self.ingest_activation(&envelope, id_present) {
            return result;
        }
        if !envelope.params.is_empty() {
            return Err(AdapterRuntimeError::Frame);
        }
        let event = match (self.state, envelope.method.as_str(), envelope.id.is_some()) {
            (_, "fixture/unknownBoundedNotification", false) => {
                vec![AdapterEvent::Unsupported(NormalizedPayload::Unsupported {
                    external_kind: SafeText::new("externalKind", "fixtureNotification")
                        .map_err(|_| AdapterRuntimeError::Frame)?,
                    summary: SafeText::new("unsupportedSummary", "Unsupported fixture event")
                        .map_err(|_| AdapterRuntimeError::Frame)?,
                })]
            }
            (_, "fixture/unknownCollaborationOperation", false) => {
                vec![AdapterEvent::Unsupported(NormalizedPayload::Unsupported {
                    external_kind: SafeText::new("externalKind", "fixtureCollaboration")
                        .map_err(|_| AdapterRuntimeError::Frame)?,
                    summary: SafeText::new("unsupportedSummary", "Unsupported collaboration event")
                        .map_err(|_| AdapterRuntimeError::Frame)?,
                })]
            }
            (_, "fixture/unknownRequest", true) => return Err(AdapterRuntimeError::Frame),
            (State::AwaitingTextStart, "fixture/textStart", false) => {
                self.state = State::Streaming;
                vec![
                    AdapterEvent::Turn {
                        id: self.identity("fixture-turn-envelope")?,
                        state: TurnState::Running,
                    },
                    self.item(ItemLifecycle::Started, "")?,
                ]
            }
            (State::Streaming, "fixture/textDelta", false) => {
                vec![self.item(ItemLifecycle::Streaming, "fixture text")?]
            }
            (State::Streaming, "fixture/textComplete", false) => {
                self.state = State::ItemComplete;
                vec![self.item(ItemLifecycle::Completed, "fixture text")?]
            }
            (State::AwaitingTextStart, "fixture/textComplete", false) => {
                self.state = State::TextComplete;
                vec![self.item(ItemLifecycle::Completed, "fixture text")?]
            }
            (State::TextComplete, "fixture/textDelta", false) => vec![AdapterEvent::IgnoredLate],
            (State::AwaitingTextStart, "fixture/toolCompletedA", false) => {
                self.state = State::ToolComplete;
                vec![self.tool(ToolState::Completed)?]
            }
            (State::ToolComplete, "fixture/toolCompletedA", false) => {
                vec![AdapterEvent::IgnoredDuplicate]
            }
            (State::ToolComplete, "fixture/toolFailedA", false) => {
                return Err(AdapterRuntimeError::Frame);
            }
            (State::ItemComplete, "fixture/turnComplete", false) => {
                self.state = State::Complete;
                vec![AdapterEvent::Turn {
                    id: self.identity("fixture-turn-envelope")?,
                    state: TurnState::Completed,
                }]
            }
            (State::AwaitingTextStart, "fixture/turnComplete", false) => {
                self.state = State::Complete;
                vec![AdapterEvent::Turn {
                    id: self.identity("fixture-turn-envelope")?,
                    state: TurnState::Completed,
                }]
            }
            (State::AwaitingTextStart, "fixture/turnCancelled", false) => {
                self.state = State::Complete;
                vec![AdapterEvent::Turn {
                    id: self.identity("fixture-turn-envelope")?,
                    state: TurnState::Cancelled,
                }]
            }
            _ => return Err(AdapterRuntimeError::Operation),
        };
        AdapterBatch::new(event, Vec::new())
    }

    fn commit_mandatory_egress(&mut self, _token: &EgressToken) -> Result<(), AdapterRuntimeError> {
        Ok(())
    }

    fn resolve_egress(
        &mut self,
        token: EgressToken,
        resolution: EgressResolution,
    ) -> Result<(), AdapterRuntimeError> {
        if token.is_cancel() {
            return self.resolve_cancel_egress(token, resolution);
        }
        if token.request_id() != self.pending_request_id {
            return Err(AdapterRuntimeError::EgressIntent);
        }
        if let Some(result) = self.resolve_activation_egress(resolution) {
            return result;
        }
        match (self.state, resolution) {
            (State::PromptCreated, EgressResolution::Queued) => {
                self.state = State::PromptQueued;
                Ok(())
            }
            (State::PromptCreated, EgressResolution::QueueRejected)
            | (State::PromptQueued, EgressResolution::WriteZeroBytes) => {
                self.pending_request_id = None;
                self.state = State::Idle;
                self.client_message_id = None;
                Ok(())
            }
            (State::PromptQueued, EgressResolution::Written) => {
                self.pending_request_id = None;
                self.state = State::AwaitingTextStart;
                Ok(())
            }
            (State::PromptQueued, EgressResolution::DeliveryUnknown) => {
                self.pending_request_id = None;
                self.state = State::Complete;
                Ok(())
            }
            _ => Err(AdapterRuntimeError::EgressIntent),
        }
    }
}

fn ingest_metadata(value: serde_json::Value) -> Result<AdapterBatch, AdapterRuntimeError> {
    if value
        .as_object()
        .is_some_and(|members| members.contains_key("id"))
    {
        return Err(AdapterRuntimeError::Frame);
    }
    let envelope =
        serde_json::from_value::<Envelope>(value).map_err(|_| AdapterRuntimeError::Frame)?;
    if envelope.jsonrpc != "2.0"
        || envelope.id.is_some()
        || envelope.method != "fixture/metadata"
        || envelope.params.len() != 1
    {
        return Err(AdapterRuntimeError::Frame);
    }
    let metadata = envelope
        .params
        .get("_meta")
        .and_then(serde_json::Value::as_object)
        .ok_or(AdapterRuntimeError::Frame)?;
    if metadata.len() > LIMITS.wire.metadata_entries as usize {
        return Err(AdapterRuntimeError::BatchBytes);
    }
    let bytes = metadata.iter().try_fold(0_usize, |total, (key, value)| {
        let value = value.as_str().ok_or(AdapterRuntimeError::Frame)?;
        total
            .checked_add(key.len())
            .and_then(|next| next.checked_add(value.len()))
            .ok_or(AdapterRuntimeError::BatchBytes)
    })?;
    if bytes > LIMITS.wire.metadata_bytes as usize {
        return Err(AdapterRuntimeError::BatchBytes);
    }
    AdapterBatch::new(vec![AdapterEvent::Acknowledged], Vec::new())
}

#[cfg(test)]
mod tests;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    jsonrpc: String,
    #[serde(default)]
    id: Option<serde_json::Value>,
    method: String,
    params: serde_json::Map<String, serde_json::Value>,
}
