use serde_json::Value;

use crate::acp::{
    bindings::adapter::{
        AdapterBatch, AdapterBatchBuilder, AdapterEvent as SharedEvent, AdapterIdentity,
        AdapterRuntimeError, ResourceMapper as SharedResourceMapper,
    },
    contract::RequiredText,
    intent::{
        CapabilitySnapshotV1, ExternalId, NormalizedPayload, SafeResourceRef,
        UnavailableResourceReason,
    },
    lifecycle::TurnState,
};

use super::super::{
    StandardAcpHandshakeError,
    sanitize::normalized_text,
    translator::{
        AdapterEvent as StandardEvent, PromptTerminal, ResourceMapper as StandardResourceMapper,
        StandardAcpTranslator,
    },
};
use super::{ActiveSession, DeferredPrompt, PendingRequest, StandardAcpRuntime, map_translation};

mod client_fs;
mod created_file;
mod path;
pub(super) mod permission;
mod safety;
pub(super) use permission::permission_egress;

impl StandardAcpRuntime {
    pub(super) fn ingest_response(
        &mut self,
        value: Value,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        let id = super::response_id(&value)?;
        let Some(pending) = self.pending.remove(&id) else {
            return self.ingest_late_prompt_response(value);
        };
        let batch = match pending {
            PendingRequest::Initialize => self.initialize_response(value),
            PendingRequest::NewSession => self.new_session_response(value),
            PendingRequest::Authenticate => self.authenticate_response(value),
            PendingRequest::Prompt => self.prompt_response(value, id),
            PendingRequest::SetMode => self.set_mode_response(value),
            PendingRequest::CloseSession => self.close_session_response(value),
        };
        if batch.is_ok() && self.delivery_unknown == Some(id) {
            self.delivery_unknown = None;
        }
        batch
    }

    fn initialize_response(&mut self, value: Value) -> Result<AdapterBatch, AdapterRuntimeError> {
        self.handshake
            .accept_initialize(&value)
            .map_err(handshake_error)?;
        self.sign_in.accept_initialize(&value);
        let mut capabilities = capabilities()?;
        capabilities.prompt_resources =
            crate::acp::intent::offers_prompt_capability(&value, "embeddedContext");
        capabilities.prompt_images = crate::acp::intent::offers_prompt_capability(&value, "image");
        if !self.profile.close_characterised {
            capabilities.close_session = value
                .pointer("/result/agentCapabilities/sessionCapabilities/close")
                .is_some_and(Value::is_object);
        }
        self.capabilities = Some(capabilities.clone());
        AdapterBatch::new(vec![SharedEvent::Initialized { capabilities }], Vec::new())
    }

    fn new_session_response(&mut self, value: Value) -> Result<AdapterBatch, AdapterRuntimeError> {
        let mode = value
            .pointer("/result/configOptions")
            .and_then(Value::as_array)
            .and_then(|options| options.iter().find(|option| option["id"] == "mode"));
        self.current_mode_id = mode
            .and_then(|mode| mode.get("currentValue"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        self.available_modes = mode
            .and_then(|mode| mode.get("options"))
            .and_then(Value::as_array)
            .map(|options| {
                options
                    .iter()
                    .filter_map(|option| option.get("value").and_then(Value::as_str))
                    .take(128)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        let external_id = match self.handshake.accept_new_session(&value) {
            Err(StandardAcpHandshakeError::SignInRequired)
                if self.profile.authenticate_on_auth_required =>
            {
                return self.sign_in_required();
            }
            accepted => accepted.map_err(handshake_error).and_then(|id| {
                ExternalId::new("externalSessionId", id).map_err(|_| AdapterRuntimeError::Frame)
            })?,
        };
        let translator = StandardAcpTranslator::new(self.profile, external_id.as_str())
            .map_err(map_translation)?;
        self.active = Some(ActiveSession {
            commands: std::sync::Arc::default(),
            external_id: external_id.clone(),
            translator,
            turn: None,
        });
        self.source.session_ready();
        AdapterBatch::new(
            vec![SharedEvent::SessionReady {
                id: self.session_identity(external_id),
                loaded: false,
            }],
            Vec::new(),
        )
    }

    fn set_mode_response(&mut self, value: Value) -> Result<AdapterBatch, AdapterRuntimeError> {
        let Some(deferred) = self.pending_prompt.take() else {
            return AdapterBatch::new(vec![SharedEvent::IgnoredDuplicate], Vec::new());
        };
        if deferred.cancelled || self.close_pending {
            return AdapterBatch::new(vec![self.deferred_cancelled(deferred)?], Vec::new());
        }
        if value.get("error").is_some() {
            return Err(AdapterRuntimeError::ConfigMutationRejected);
        }
        if !value.get("result").is_some_and(Value::is_object) {
            return Err(AdapterRuntimeError::Frame);
        }
        self.current_mode_id = Some(deferred.mode_id);
        let egress = self.prompt(
            deferred.prompt,
            deferred.client_message_id,
            deferred.access_mode,
        )?;
        AdapterBatch::new(Vec::new(), vec![egress])
    }

    fn deferred_cancelled(
        &self,
        deferred: DeferredPrompt,
    ) -> Result<SharedEvent, AdapterRuntimeError> {
        let active = self.active.as_ref().ok_or(AdapterRuntimeError::Frame)?;
        let turn_id = self.turn_external_id("deferred", deferred.request_id)?;
        Ok(SharedEvent::Turn {
            id: identity(
                self.binding_digest,
                self.runtime_epoch,
                &self.session_id,
                active.external_id.clone(),
                Some(turn_id),
                Some(deferred.client_message_id),
            ),
            state: TurnState::Cancelled,
        })
    }

    fn close_session_response(
        &mut self,
        value: Value,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        if value.get("error").is_some() || !value.get("result").is_some_and(Value::is_object) {
            return Err(AdapterRuntimeError::Frame);
        }
        let mut events = Vec::new();
        if let Some(deferred) = self.pending_prompt.take() {
            events.push(self.deferred_cancelled(deferred)?);
        }
        if self
            .active
            .as_ref()
            .is_some_and(|active| active.turn.is_some())
        {
            self.close_pending = true;
        } else {
            self.active = None;
        }
        events.push(SharedEvent::Acknowledged);
        AdapterBatch::new(events, Vec::new())
    }

    fn prompt_response(
        &mut self,
        value: Value,
        id: u64,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        if self.safety_alerted {
            return AdapterBatch::new(vec![SharedEvent::IgnoredLate], Vec::new());
        }
        let active = self.active.as_mut().ok_or(AdapterRuntimeError::Frame)?;
        if active.turn.as_ref().map(|turn| turn.request_id) != Some(id) {
            return Err(AdapterRuntimeError::Frame);
        }
        let events = active
            .translator
            .translate_prompt_result_value(value)
            .map_err(map_translation)?;
        let mut all = std::mem::take(&mut self.deferred);
        all.extend(events);
        self.translate(all)
    }

    fn ingest_late_prompt_response(
        &mut self,
        value: Value,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        let active = self.active.as_mut().ok_or(AdapterRuntimeError::Frame)?;
        let events = active
            .translator
            .translate_prompt_result_value(value)
            .map_err(map_translation)?;
        let mut all = std::mem::take(&mut self.deferred);
        all.extend(events);
        self.translate(all)
    }

    fn translate(
        &mut self,
        events: Vec<StandardEvent>,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        let mut batch = AdapterBatchBuilder::new();
        let mut terminal = false;
        for event in events {
            match event {
                StandardEvent::Item {
                    external_key,
                    lifecycle,
                    tool_state,
                    payload,
                } => batch.push_event(SharedEvent::Item {
                    id: self.item_identity(external_key)?,
                    lifecycle,
                    tool_state,
                    payload,
                })?,
                StandardEvent::FilePair {
                    external_key,
                    old_text,
                    new_text,
                } => batch.push_event(SharedEvent::FilePair {
                    id: self.item_identity(external_key.clone())?,
                    external_item_id: external_key,
                    old_text,
                    new_text,
                })?,
                StandardEvent::TurnRunning => batch.push_event(SharedEvent::Turn {
                    id: self.turn_identity()?,
                    state: TurnState::Running,
                })?,
                StandardEvent::TurnTerminal(terminal_state) => {
                    batch.push_event(SharedEvent::Turn {
                        id: self.turn_identity()?,
                        state: turn_state(terminal_state),
                    })?;
                    terminal = true;
                }
                StandardEvent::Usage(payload) => batch.push_event(SharedEvent::Usage {
                    turn_id: self.turn_identity()?,
                    payload,
                })?,
                StandardEvent::Absorbed => batch.push_event(SharedEvent::Absorbed)?,
                StandardEvent::IgnoredDuplicate => {
                    batch.push_event(SharedEvent::IgnoredDuplicate)?
                }
                StandardEvent::IgnoredLate => batch.push_event(SharedEvent::IgnoredLate)?,
                StandardEvent::PermissionDecline { .. } => {
                    return Err(AdapterRuntimeError::MandatoryDenyTeardown);
                }
            }
        }
        if terminal {
            if self.close_pending {
                self.active = None;
                self.close_pending = false;
            } else if let Some(active) = self.active.as_mut() {
                active.turn = None;
            }
        }
        Ok(batch.finish())
    }

    fn session_identity(&self, external_id: ExternalId) -> AdapterIdentity {
        identity(
            self.binding_digest,
            self.runtime_epoch,
            &self.session_id,
            external_id,
            None,
            None,
        )
    }

    pub(super) fn turn_identity(&self) -> Result<AdapterIdentity, AdapterRuntimeError> {
        let active = self.active.as_ref().ok_or(AdapterRuntimeError::Frame)?;
        let turn = active.turn.as_ref().ok_or(AdapterRuntimeError::Frame)?;
        Ok(identity(
            self.binding_digest,
            self.runtime_epoch,
            &self.session_id,
            active.external_id.clone(),
            Some(turn.external_id.clone()),
            Some(turn.client_message_id.clone()),
        ))
    }

    pub(super) fn item_identity(
        &self,
        external_id: ExternalId,
    ) -> Result<AdapterIdentity, AdapterRuntimeError> {
        let mut identity = self.turn_identity()?;
        identity.external_id = external_id;
        Ok(identity)
    }
}

struct MapperBridge<'a>(&'a dyn SharedResourceMapper);

impl StandardResourceMapper for MapperBridge<'_> {
    fn map_absolute(
        &self,
        absolute_path: &str,
    ) -> Result<SafeResourceRef, UnavailableResourceReason> {
        self.0.map_existing(absolute_path)
    }

    fn map_missing_display(&self, absolute_path: &str) -> Option<SafeResourceRef> {
        self.0.map_missing_display(absolute_path)
    }
}

fn capabilities() -> Result<CapabilitySnapshotV1, AdapterRuntimeError> {
    Ok(CapabilitySnapshotV1 {
        schema_version: crate::acp::contract::SchemaVersionV1,
        protocol_version: RequiredText::new("protocolVersion", "1")
            .map_err(|_| AdapterRuntimeError::Frame)?,
        new_session: true,
        resume_session: false,
        load_session: false,
        close_session: true,
        prompt: true,
        cancel: true,
        native_session_fork: false,
        native_session_compact: false,
        native_session_import: false,
        native_review: false,
        native_background_terminals: false,
        active_turn_steer: false,
        child_conversation_projection: false,
        child_transcript_read: false,
        collaboration_event_projection: false,
        multi_root_extension_schema_id: None,
        model_count: 0,
        prompt_resources: false,
        prompt_images: false,
        config_count: 0,
    })
}

fn handshake_error(error: StandardAcpHandshakeError) -> AdapterRuntimeError {
    match error {
        StandardAcpHandshakeError::SignInRequired => AdapterRuntimeError::SignInRequired,
        StandardAcpHandshakeError::Frame | StandardAcpHandshakeError::Operation => {
            AdapterRuntimeError::Frame
        }
    }
}

fn identity(
    binding_digest: [u8; 32],
    runtime_epoch: crate::acp::contract::RuntimeEpoch,
    session_id: &crate::acp::contract::SageAgentSessionId,
    external_id: ExternalId,
    turn_id: Option<ExternalId>,
    turn_client_message_id: Option<RequiredText<512>>,
) -> AdapterIdentity {
    AdapterIdentity {
        binding_digest,
        runtime_epoch,
        session_id: session_id.clone(),
        external_id,
        turn_id,
        turn_client_message_id,
    }
}

fn turn_state(terminal: PromptTerminal) -> TurnState {
    match terminal {
        PromptTerminal::Completed => TurnState::Completed,
        PromptTerminal::Cancelled => TurnState::Cancelled,
        PromptTerminal::Failed => TurnState::Failed,
    }
}

fn unsupported(label: &str, method: &str) -> Result<NormalizedPayload, AdapterRuntimeError> {
    Ok(NormalizedPayload::Unsupported {
        external_kind: normalized_text("externalKind", method)
            .map_err(|_| AdapterRuntimeError::Frame)?,
        summary: normalized_text(
            "unsupportedSummary",
            &format!("{label} ACP message is outside the mapped standard ACP surface"),
        )
        .map_err(|_| AdapterRuntimeError::Frame)?,
    })
}
