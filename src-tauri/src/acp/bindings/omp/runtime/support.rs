use crate::acp::{
    bindings::adapter::{
        AdapterBatch, AdapterBatchBuilder, AdapterEgress, AdapterEvent as RuntimeEvent,
        AdapterRuntimeError, ResourceMapper,
    },
    contract::{RequiredText, SchemaVersionV1},
    intent::{CapabilitySnapshotV1, ExternalId, NormalizedPayload},
    lifecycle::TurnState,
    transport::OutboundFrame,
};

use super::{OmpRuntime, RequestKind};
use crate::acp::bindings::omp::{
    AdapterEvent, ModelSummary, Operation, PromptTerminal, TranslationError,
};

#[path = "support/delivery.rs"]
mod delivery;
#[path = "support/identity.rs"]
mod identity;

pub(super) struct MapperBridge<'a>(pub(super) &'a dyn ResourceMapper);

impl OmpRuntime {
    pub(super) fn resolve_later_permission(
        &mut self,
        request_id: ExternalId,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        if self.translator.pending_count() == 0 && !self.denials.contains_key(request_id.as_str()) {
            return Err(AdapterRuntimeError::PermissionIntent);
        }
        self.prepare_deny(request_id)?
            .ok_or(AdapterRuntimeError::Operation)
    }
}

pub(super) fn outbound_frame(line: &str) -> Result<OutboundFrame, AdapterRuntimeError> {
    let payload = line.strip_suffix('\n').ok_or(AdapterRuntimeError::Frame)?;
    OutboundFrame::new(payload.as_bytes().to_vec()).map_err(|_| AdapterRuntimeError::Frame)
}

impl crate::acp::bindings::omp::ResourceMapper for MapperBridge<'_> {
    fn map_absolute(
        &self,
        absolute_path: &str,
    ) -> Result<crate::acp::intent::SafeResourceRef, crate::acp::intent::UnavailableResourceReason>
    {
        self.0.map_existing(absolute_path)
    }
}

pub(super) fn capabilities(runtime: &OmpRuntime) -> CapabilitySnapshotV1 {
    CapabilitySnapshotV1 {
        schema_version: SchemaVersionV1,
        protocol_version: RequiredText::new(
            "protocolVersion",
            format!("omp-acp-{}", runtime.protocol_version.unwrap_or(1)),
        )
        .expect("bounded protocol version"),
        new_session: true,
        resume_session: false,
        load_session: runtime.negotiated_load,
        close_session: false,
        prompt: true,
        cancel: true,
        native_session_fork: false,
        active_turn_steer: false,
        child_conversation_projection: false,
        child_transcript_read: false,
        collaboration_event_projection: false,
        multi_root_extension_schema_id: None,
        model_count: runtime.model_count,
        config_count: 0,
    }
}

impl OmpRuntime {
    pub(super) fn snapshot(&self) -> CapabilitySnapshotV1 {
        capabilities(self)
    }

    pub(super) fn translate_events(
        &mut self,
        events: Vec<AdapterEvent>,
        request_digest: [u8; 32],
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        let mut builder = AdapterBatchBuilder::new();
        for event in events {
            let (normalized, egress) = match self.translate_event(event, request_digest) {
                Ok(value) => value,
                Err(error) => {
                    self.poison();
                    return Err(error);
                }
            };
            for event in normalized {
                if let Err(error) = builder.push_event(event) {
                    self.poison();
                    return Err(error);
                }
            }
            if let Some(egress) = egress
                && let Err(error) = builder.push_egress(egress)
            {
                self.poison();
                return Err(if self.denials.is_empty() {
                    error
                } else {
                    AdapterRuntimeError::MandatoryDenyTeardown
                });
            }
        }
        Ok(builder.finish())
    }

    fn translate_event(
        &mut self,
        event: AdapterEvent,
        request_digest: [u8; 32],
    ) -> Result<(Vec<RuntimeEvent>, Option<AdapterEgress>), AdapterRuntimeError> {
        let (events, egress) = match event {
            AdapterEvent::Initialized {
                protocol_version,
                load_session,
            } => {
                self.protocol_version = Some(protocol_version);
                self.negotiated_load = load_session;
                self.initialized = true;
                (
                    vec![RuntimeEvent::Initialized {
                        capabilities: self.snapshot(),
                    }],
                    None,
                )
            }
            AdapterEvent::SessionReady {
                external_session_id,
                models,
            } => {
                let transition = self
                    .transition
                    .take()
                    .ok_or(AdapterRuntimeError::Operation)?;
                let models_event = self.models_event(models)?;
                self.external_session = Some(external_session_id.clone());
                self.native_root = Some(transition.native_root);
                self.active_turn = None;
                self.snapshots.clear();
                (
                    vec![
                        models_event,
                        RuntimeEvent::SessionReady {
                            id: self.identity(external_session_id, None, None),
                            loaded: transition.loaded,
                        },
                    ],
                    None,
                )
            }
            AdapterEvent::TurnRunning { turn_key } => {
                self.require_turn(&turn_key)?;
                (
                    vec![RuntimeEvent::Turn {
                        id: self.turn_identity()?,
                        state: TurnState::Running,
                    }],
                    None,
                )
            }
            AdapterEvent::Item {
                external_key,
                lifecycle,
                tool_state,
                payload,
                update_mode,
            } => {
                let payload =
                    match self
                        .snapshots
                        .apply(&external_key, lifecycle, update_mode, payload)
                    {
                        Ok(payload) => payload,
                        Err(error) => {
                            self.poison();
                            return Err(error);
                        }
                    };
                let event = match payload {
                    NormalizedPayload::Usage { .. } => RuntimeEvent::Usage {
                        turn_id: self.turn_identity()?,
                        payload,
                    },
                    NormalizedPayload::PermissionRequest { .. } => {
                        RuntimeEvent::PermissionRequest {
                            turn_id: self.turn_identity()?,
                            item_id: self.item_identity(external_key.clone())?,
                            external_request_id: external_key,
                            payload,
                            request_payload_digest: request_digest,
                        }
                    }
                    _ => RuntimeEvent::Item {
                        id: self.item_identity(external_key)?,
                        lifecycle,
                        tool_state,
                        payload,
                    },
                };
                (vec![event], None)
            }
            AdapterEvent::FilePair {
                external_key,
                old_text,
                new_text,
                ..
            } => {
                let event = RuntimeEvent::FilePair {
                    id: self.item_identity(external_key.clone())?,
                    external_item_id: external_key,
                    old_text,
                    new_text,
                };
                (vec![event], None)
            }
            AdapterEvent::PermissionDeclinePending { request_id, .. } => {
                let egress = self.prepare_deny(request_id)?;
                let duplicate = egress.is_none().then_some(RuntimeEvent::IgnoredDuplicate);
                (duplicate.into_iter().collect(), egress)
            }
            AdapterEvent::ToolAuthorityDenied { .. } => (Vec::new(), None),
            AdapterEvent::CancelDispatchRequired => {
                let egress = self.authority_cancel()?;
                (Vec::new(), egress)
            }
            AdapterEvent::TurnTerminal { turn_key, terminal } => {
                self.require_turn(&turn_key)?;
                let state = match terminal {
                    PromptTerminal::Completed => TurnState::Completed,
                    PromptTerminal::Cancelled => TurnState::Cancelled,
                    PromptTerminal::Failed => TurnState::Failed,
                };
                let event = RuntimeEvent::Turn {
                    id: self.turn_identity()?,
                    state,
                };
                self.active_turn = None;
                self.snapshots.clear();
                (vec![event], None)
            }
            AdapterEvent::IgnoredDuplicate => (vec![RuntimeEvent::IgnoredDuplicate], None),
            AdapterEvent::IgnoredLate => (vec![RuntimeEvent::IgnoredLate], None),
        };
        Ok((events, egress))
    }

    fn authority_cancel(&mut self) -> Result<Option<AdapterEgress>, AdapterRuntimeError> {
        let mut values = self.translator.begin(Operation::Cancel).map_err(map_omp)?;
        if values.is_empty() {
            return Ok(None);
        }
        if values.len() != 1 {
            self.poison();
            return Err(AdapterRuntimeError::MandatoryDenyTeardown);
        }
        match self.reserve(values.remove(0), RequestKind::AuthorityCancel) {
            Ok(egress) => Ok(Some(egress)),
            Err(_) => {
                self.poison();
                Err(AdapterRuntimeError::MandatoryDenyTeardown)
            }
        }
    }

    pub(super) fn poison(&mut self) {
        self.poisoned = true;
        self.transition = None;
        self.snapshots.clear();
    }
}

pub(super) fn map_omp(error: TranslationError) -> AdapterRuntimeError {
    match error {
        TranslationError::UnsupportedOperation(_) => AdapterRuntimeError::UnsupportedCommand,
        TranslationError::RequestCapacity => AdapterRuntimeError::BatchCount,
        TranslationError::MalformedFrame
        | TranslationError::Limit(_)
        | TranslationError::UnexpectedMethod
        | TranslationError::NativePath
        | TranslationError::Resource
        | TranslationError::Wire(_) => AdapterRuntimeError::Frame,
        _ => AdapterRuntimeError::Operation,
    }
}
