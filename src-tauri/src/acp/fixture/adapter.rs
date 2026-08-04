use serde::Deserialize;

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

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Idle,
    PromptCreated,
    PromptQueued,
    AwaitingTextStart,
    Streaming,
    ItemComplete,
    ToolComplete,
    TextComplete,
    Complete,
}

pub(super) struct FixtureAdapter {
    binding_digest: [u8; 32],
    runtime_epoch: RuntimeEpoch,
    session_id: SageAgentSessionId,
    client_message_id: Option<RequiredText<512>>,
    state: State,
}

impl FixtureAdapter {
    // The constructor is consumed by the external feature-only runner; the
    // default library build deliberately has no admitted fixture binding.
    #[allow(dead_code)]
    pub(super) fn new(
        binding_digest: [u8; 32],
        runtime_epoch: RuntimeEpoch,
        session_id: SageAgentSessionId,
        _external_session_id: ExternalId,
    ) -> Result<Self, AdapterRuntimeError> {
        if binding_digest == [0; 32] || runtime_epoch == RuntimeEpoch::ZERO {
            return Err(AdapterRuntimeError::Operation);
        }
        Ok(Self {
            binding_digest,
            runtime_epoch,
            session_id,
            client_message_id: None,
            state: State::Idle,
        })
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
            payload: NormalizedPayload::ToolUpdateResult {
                category: crate::acp::intent::ToolCategory::Other,
                label: SafeText::new("toolLabel", "Fixture tool")
                    .map_err(|_| AdapterRuntimeError::Frame)?,
                output_summary: None,
            },
        })
    }
}

impl HarnessAdapter for FixtureAdapter {
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
        if self.state != State::Idle {
            return Err(AdapterRuntimeError::Operation);
        }
        self.client_message_id = Some(client_message_id);
        self.state = State::PromptCreated;
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
        let envelope: Envelope =
            serde_json::from_slice(frame.as_bytes()).map_err(|_| AdapterRuntimeError::Frame)?;
        if envelope.jsonrpc != "2.0" || !envelope.params.is_empty() {
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
        if token.request_id() != Some(1) {
            return Err(AdapterRuntimeError::EgressIntent);
        }
        match (self.state, resolution) {
            (State::PromptCreated, EgressResolution::Queued) => {
                self.state = State::PromptQueued;
                Ok(())
            }
            (State::PromptCreated, EgressResolution::QueueRejected)
            | (State::PromptQueued, EgressResolution::WriteZeroBytes) => {
                self.state = State::Idle;
                self.client_message_id = None;
                Ok(())
            }
            (State::PromptQueued, EgressResolution::Written) => {
                self.state = State::AwaitingTextStart;
                Ok(())
            }
            (State::PromptQueued, EgressResolution::DeliveryUnknown) => {
                self.state = State::Complete;
                Ok(())
            }
            _ => Err(AdapterRuntimeError::EgressIntent),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    jsonrpc: String,
    #[serde(default)]
    id: Option<serde_json::Value>,
    method: String,
    params: serde_json::Map<String, serde_json::Value>,
}
