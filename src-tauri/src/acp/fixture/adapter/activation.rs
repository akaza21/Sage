use crate::acp::{
    bindings::adapter::{
        AdapterBatch, AdapterCommand, AdapterEgress, AdapterEvent, AdapterRuntimeError,
        EgressResolution,
    },
    contract::{RequiredText, SafeText, SchemaVersionV1},
    intent::{CapabilitySnapshotV1, NormalizedPayload},
};

use super::{Envelope, FixtureAdapter, State};

impl FixtureAdapter {
    pub(in crate::acp::fixture) fn activation_capabilities() -> CapabilitySnapshotV1 {
        CapabilitySnapshotV1 {
            schema_version: SchemaVersionV1,
            protocol_version: RequiredText::new("protocol", "fixture-1").unwrap(),
            new_session: true,
            resume_session: true,
            load_session: false,
            close_session: true,
            prompt: true,
            cancel: true,
            native_session_fork: false,
            active_turn_steer: false,
            child_conversation_projection: false,
            child_transcript_read: false,
            collaboration_event_projection: false,
            multi_root_extension_schema_id: None,
            model_count: 0,
            config_count: 0,
        }
    }

    pub(super) fn activation_command(
        &mut self,
        command: AdapterCommand,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        match command {
            AdapterCommand::Initialize if self.state == State::Idle && !self.session_ready => {
                let request = self.request(
                    br#"{"jsonrpc":"2.0","method":"fixture/initialize","params":{}}"#.to_vec(),
                )?;
                self.state = State::InitializeCreated;
                Ok(request)
            }
            AdapterCommand::ResumeSession {
                external_session_id,
                ..
            } if self.state == State::Initialized
                && external_session_id == self.external_session_id =>
            {
                let payload = serde_json::to_vec(&serde_json::json!({
                    "jsonrpc": "2.0",
                    "method": "fixture/resume",
                    "params": {"externalSessionId": external_session_id.as_str()},
                }))
                .map_err(|_| AdapterRuntimeError::Frame)?;
                let request = self.request(payload)?;
                self.state = State::ResumeCreated;
                Ok(request)
            }
            AdapterCommand::Initialize | AdapterCommand::ResumeSession { .. } => {
                Err(AdapterRuntimeError::Operation)
            }
            _ => Err(AdapterRuntimeError::UnsupportedCommand),
        }
    }

    pub(super) fn ingest_activation(
        &mut self,
        envelope: &Envelope,
    ) -> Option<Result<AdapterBatch, AdapterRuntimeError>> {
        let result = match envelope.method.as_str() {
            "fixture/initializeSuccess" => self.ingest_initialized(envelope),
            "fixture/resumeSuccess" => self.ingest_resume_success(envelope),
            "fixture/resumeFailure" => self.ingest_resume_failure(envelope),
            _ => return None,
        };
        Some(result)
    }

    pub(super) fn resolve_activation_egress(
        &mut self,
        resolution: EgressResolution,
    ) -> Option<Result<(), AdapterRuntimeError>> {
        let next = match (self.state, resolution) {
            (State::InitializeCreated, EgressResolution::Queued) => State::InitializeQueued,
            (State::InitializeCreated, EgressResolution::QueueRejected)
            | (State::InitializeQueued, EgressResolution::WriteZeroBytes) => State::Idle,
            (State::InitializeQueued, EgressResolution::Written) => State::AwaitingInitialize,
            (State::ResumeCreated, EgressResolution::Queued) => State::ResumeQueued,
            (State::ResumeCreated, EgressResolution::QueueRejected)
            | (State::ResumeQueued, EgressResolution::WriteZeroBytes) => State::Initialized,
            (State::ResumeQueued, EgressResolution::Written) => State::AwaitingResume,
            (State::InitializeQueued | State::ResumeQueued, EgressResolution::DeliveryUnknown) => {
                State::Complete
            }
            (
                State::InitializeCreated
                | State::InitializeQueued
                | State::ResumeCreated
                | State::ResumeQueued,
                _,
            ) => return Some(Err(AdapterRuntimeError::EgressIntent)),
            _ => return None,
        };
        if resolution != EgressResolution::Queued {
            self.pending_request_id = None;
        }
        self.state = next;
        Some(Ok(()))
    }

    fn ingest_initialized(
        &mut self,
        envelope: &Envelope,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        if self.state != State::AwaitingInitialize
            || envelope.id.is_some()
            || !envelope.params.is_empty()
        {
            return Err(AdapterRuntimeError::Operation);
        }
        let capabilities = Self::activation_capabilities();
        self.capabilities = Some(capabilities.clone());
        self.state = State::Initialized;
        AdapterBatch::new(vec![AdapterEvent::Initialized { capabilities }], Vec::new())
    }

    fn ingest_resume_success(
        &mut self,
        envelope: &Envelope,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        if self.state != State::AwaitingResume
            || envelope.id.is_some()
            || envelope.params.len() != 1
            || envelope
                .params
                .get("externalSessionId")
                .and_then(serde_json::Value::as_str)
                != Some(self.external_session_id.as_str())
        {
            return Err(AdapterRuntimeError::Operation);
        }
        self.session_ready = true;
        self.state = State::Idle;
        AdapterBatch::new(
            vec![AdapterEvent::SessionReady {
                id: self.session_identity(),
                loaded: false,
            }],
            Vec::new(),
        )
    }

    fn ingest_resume_failure(
        &mut self,
        envelope: &Envelope,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        if self.state != State::AwaitingResume
            || envelope.id.is_some()
            || !envelope.params.is_empty()
        {
            return Err(AdapterRuntimeError::Operation);
        }
        self.state = State::Complete;
        AdapterBatch::new(
            vec![AdapterEvent::Error(NormalizedPayload::Error {
                code: SafeText::new("errorCode", "fixtureResumeFailed")
                    .map_err(|_| AdapterRuntimeError::Frame)?,
                message: SafeText::new("errorMessage", "Fixture resume failed")
                    .map_err(|_| AdapterRuntimeError::Frame)?,
                retryable: false,
            })],
            Vec::new(),
        )
    }
}
