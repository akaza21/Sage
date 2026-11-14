use std::collections::BTreeMap;

use crate::acp::{
    bindings::adapter::{
        AdapterBatch, AdapterCommand, AdapterEgress, AdapterRuntimeError, EgressResolution,
        EgressToken, HarnessAdapter, ResourceMapper,
    },
    contract::{RequiredText, RuntimeEpoch, SageAgentSessionId},
    intent::{CapabilitySnapshotV1, ExternalId},
    transport::{Frame, request_payload_digest},
};
use sage_core::limits::LIMITS;

use super::{OmpBindingCapabilities, OmpTranslator, Operation, Outbound, ResearchBinding};

mod support;

mod admission;
mod read_text_file;
mod snapshots;
use snapshots::ItemSnapshots;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RequestKind {
    Initialize,
    NewSession,
    LoadSession,
    Prompt,
    Cancel,
    AuthorityCancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeliveryPhase {
    Reserved,
    Queued,
}

struct RequestReservation {
    command_id: Option<String>,
    kind: RequestKind,
    phase: DeliveryPhase,
}

pub(super) struct TurnIntent {
    pub(super) external_id: ExternalId,
    pub(super) client_message_id: RequiredText<512>,
}

pub(super) struct SessionTransition {
    pub(super) native_root: String,
    pub(super) loaded: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DenyStage {
    Prepared,
    Committed,
    Queued,
}

pub(super) struct DenyIntent {
    pub(super) replay: bool,
    pub(super) cancelled: bool,
    pub(super) stage: DenyStage,
}

pub(crate) struct OmpRuntime {
    pub(super) translator: OmpTranslator,
    pub(super) binding_digest: [u8; 32],
    pub(super) runtime_epoch: RuntimeEpoch,
    pub(super) session_id: SageAgentSessionId,
    pub(super) external_session: Option<ExternalId>,
    pub(super) native_root: Option<String>,
    pub(super) transition: Option<SessionTransition>,
    pub(super) active_turn: Option<TurnIntent>,
    pub(super) denials: BTreeMap<String, DenyIntent>,
    pub(super) deferred: Vec<super::AdapterEvent>,
    snapshots: ItemSnapshots,
    reservations: BTreeMap<u64, RequestReservation>,
    next_egress: u64,
    pub(super) initialized: bool,
    pub(super) negotiated_load: bool,
    pub(super) protocol_version: Option<u64>,
    pub(super) model_count: u16,
    pub(super) current_model: Option<String>,
    pub(super) poisoned: bool,
}

impl OmpRuntime {
    #[cfg(test)]
    pub(crate) fn fixture(
        capabilities: OmpBindingCapabilities,
        runtime_epoch: RuntimeEpoch,
        session_id: SageAgentSessionId,
    ) -> Self {
        Self::new(capabilities, [11; 32], runtime_epoch, session_id)
    }

    fn new(
        capabilities: OmpBindingCapabilities,
        binding_digest: [u8; 32],
        runtime_epoch: RuntimeEpoch,
        session_id: SageAgentSessionId,
    ) -> Self {
        Self {
            translator: OmpTranslator::new(runtime_epoch.get(), capabilities),
            binding_digest,
            runtime_epoch,
            session_id,
            external_session: None,
            native_root: None,
            transition: None,
            active_turn: None,
            denials: BTreeMap::new(),
            deferred: Vec::new(),
            snapshots: ItemSnapshots::new(),
            reservations: BTreeMap::new(),
            next_egress: 0,
            initialized: false,
            negotiated_load: false,
            protocol_version: None,
            model_count: 0,
            current_model: None,
            poisoned: false,
        }
    }

    fn begin_one(
        &mut self,
        operation: Operation,
        kind: RequestKind,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        let mut values = self.translator.begin(operation).map_err(support::map_omp)?;
        if values.len() != 1 {
            return Err(AdapterRuntimeError::Operation);
        }
        self.reserve(values.remove(0), kind)
    }

    pub(super) fn reserve(
        &mut self,
        outbound: Outbound,
        kind: RequestKind,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        if self.reservations.len() >= LIMITS.queues.outstanding_rpc_per_session as usize {
            self.reject_outbound(&outbound, kind)?;
            return Err(AdapterRuntimeError::BatchCount);
        }
        let frame = match support::outbound_frame(&outbound.line) {
            Ok(frame) => frame,
            Err(_) => {
                self.reject_outbound(&outbound, kind)?;
                return Err(AdapterRuntimeError::Frame);
            }
        };
        let next = match self
            .next_egress
            .checked_add(1)
            .filter(|value| *value <= i64::MAX as u64)
        {
            Some(value) => value,
            None => {
                self.reject_outbound(&outbound, kind)?;
                return Err(AdapterRuntimeError::Operation);
            }
        };
        self.next_egress = next;
        self.reservations.insert(
            next,
            RequestReservation {
                command_id: outbound.request_id,
                kind,
                phase: DeliveryPhase::Reserved,
            },
        );
        let token = if matches!(kind, RequestKind::Cancel | RequestKind::AuthorityCancel) {
            EgressToken::cancel_request(next)
        } else {
            EgressToken::request(next)
        };
        Ok(AdapterEgress::new(frame, token))
    }

    fn rollback(&mut self, id: u64) -> Result<(), AdapterRuntimeError> {
        let reservation = self
            .reservations
            .remove(&id)
            .ok_or(AdapterRuntimeError::EgressIntent)?;
        if reservation.kind == RequestKind::AuthorityCancel {
            self.poisoned = true;
            return Err(AdapterRuntimeError::MandatoryDenyTeardown);
        }
        match reservation.command_id.as_deref() {
            Some(command_id) => self
                .translator
                .reject_before_write(command_id)
                .map_err(support::map_omp)?,
            None if reservation.kind == RequestKind::Cancel => self
                .translator
                .reject_cancel_before_write()
                .map_err(support::map_omp)?,
            None => return Err(AdapterRuntimeError::EgressIntent),
        }
        match reservation.kind {
            RequestKind::Initialize => self.initialized = false,
            RequestKind::NewSession | RequestKind::LoadSession => self.transition = None,
            RequestKind::Prompt => {
                self.active_turn = None;
                self.snapshots.clear();
                self.deferred.clear();
            }
            RequestKind::Cancel => {}
            RequestKind::AuthorityCancel => unreachable!(),
        }
        Ok(())
    }
}

impl HarnessAdapter for OmpRuntime {
    fn capabilities(&self) -> Option<CapabilitySnapshotV1> {
        self.initialized.then(|| self.snapshot())
    }

    fn command(&mut self, command: AdapterCommand) -> Result<AdapterEgress, AdapterRuntimeError> {
        if self.poisoned {
            return Err(AdapterRuntimeError::Operation);
        }
        match command {
            AdapterCommand::Steer { .. } => Err(AdapterRuntimeError::UnsupportedCommand),
            AdapterCommand::Initialize => {
                self.begin_one(Operation::Initialize, RequestKind::Initialize)
            }
            AdapterCommand::ModelPage { .. } => Err(AdapterRuntimeError::UnsupportedCommand),
            AdapterCommand::NewSession { native_root, model } if self.initialized => {
                self.validate_model(model.as_ref())?;
                let egress = self.begin_one(
                    Operation::NewSession {
                        native_cwd: native_root.clone(),
                        root_count: 1,
                    },
                    RequestKind::NewSession,
                )?;
                self.transition = Some(SessionTransition {
                    native_root,
                    loaded: false,
                });
                Ok(egress)
            }
            AdapterCommand::NewSession { .. } => Err(AdapterRuntimeError::Operation),
            AdapterCommand::ResumeSession { .. } | AdapterCommand::CloseSession => {
                Err(AdapterRuntimeError::UnsupportedCommand)
            }
            AdapterCommand::LoadSession {
                external_session_id,
            } if self.initialized && self.negotiated_load => {
                let native_root = self
                    .native_root
                    .clone()
                    .ok_or(AdapterRuntimeError::Operation)?;
                let egress = self.begin_one(
                    Operation::LoadSession {
                        external_session_id: external_session_id.as_str().to_owned(),
                        native_cwd: native_root.clone(),
                        root_count: 1,
                    },
                    RequestKind::LoadSession,
                )?;
                self.transition = Some(SessionTransition {
                    native_root,
                    loaded: true,
                });
                Ok(egress)
            }
            AdapterCommand::LoadSession { .. } => Err(AdapterRuntimeError::UnsupportedCommand),
            AdapterCommand::Prompt {
                prompt,
                client_message_id,
                ..
            } => {
                if self.external_session.is_none() || self.active_turn.is_some() {
                    return Err(AdapterRuntimeError::Operation);
                }
                let external_id =
                    ExternalId::new("ompTurnId", client_message_id.as_str().to_owned())
                        .map_err(|_| AdapterRuntimeError::Operation)?;
                let egress = self.begin_one(
                    Operation::Prompt {
                        turn_key: external_id.as_str().to_owned(),
                        message: prompt.as_str().to_owned(),
                    },
                    RequestKind::Prompt,
                )?;
                self.active_turn = Some(TurnIntent {
                    external_id,
                    client_message_id,
                });
                self.snapshots.clear();
                Ok(egress)
            }
            AdapterCommand::Cancel if self.active_turn.is_some() => {
                self.begin_one(Operation::Cancel, RequestKind::Cancel)
            }
            AdapterCommand::Cancel => Err(AdapterRuntimeError::Operation),
            AdapterCommand::ResolvePermission { request_id, .. } => self.resolve_later_permission(
                ExternalId::new("permissionRequest", request_id.as_str())
                    .map_err(|_| AdapterRuntimeError::PermissionIntent)?,
            ),
        }
    }

    fn cancel_permission(
        &mut self,
        request_id: &crate::acp::contract::SageServerRequestId,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        self.prepare_permission_cancel(request_id)
    }

    fn ingest(
        &mut self,
        frame: &Frame,
        resources: &dyn ResourceMapper,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        if self.poisoned {
            return Err(AdapterRuntimeError::Operation);
        }
        let mapper = support::MapperBridge(resources);
        let events = match self.translator.ingest(frame.as_bytes(), &mapper) {
            Ok(events) => events,
            Err(error) => {
                self.poisoned = true;
                self.transition = None;
                return Err(support::map_omp(error));
            }
        };
        let request_digest =
            request_payload_digest(frame).map_err(|_| AdapterRuntimeError::Frame)?;
        self.translate_events(events, request_digest)
    }

    fn commit_mandatory_egress(&mut self, token: &EgressToken) -> Result<(), AdapterRuntimeError> {
        self.commit_deny(token)
    }

    fn resolve_egress(
        &mut self,
        token: EgressToken,
        resolution: EgressResolution,
    ) -> Result<(), AdapterRuntimeError> {
        if token.deny_request().is_some() || token.cancelled_permission_request().is_some() {
            return self.resolve_deny(token, resolution);
        }
        let id = token
            .request_id()
            .ok_or(AdapterRuntimeError::EgressIntent)?;
        match resolution {
            EgressResolution::QueueRejected | EgressResolution::WriteZeroBytes => self.rollback(id),
            EgressResolution::Queued => {
                let reservation = self
                    .reservations
                    .get_mut(&id)
                    .ok_or(AdapterRuntimeError::EgressIntent)?;
                if reservation.phase != DeliveryPhase::Reserved {
                    return Err(AdapterRuntimeError::EgressIntent);
                }
                reservation.phase = DeliveryPhase::Queued;
                Ok(())
            }
            EgressResolution::Written => {
                let reservation = self
                    .reservations
                    .get(&id)
                    .ok_or(AdapterRuntimeError::EgressIntent)?;
                if reservation.phase != DeliveryPhase::Queued {
                    return Err(AdapterRuntimeError::EgressIntent);
                }
                self.reservations.remove(&id);
                Ok(())
            }
            EgressResolution::DeliveryUnknown => {
                self.reservations
                    .remove(&id)
                    .ok_or(AdapterRuntimeError::EgressIntent)?;
                self.transition = None;
                self.poisoned = true;
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests;
