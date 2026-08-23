use std::collections::BTreeMap;

use crate::acp::{
    bindings::adapter::{
        AdapterBatch, AdapterCommand, AdapterEgress, AdapterRuntimeError, EgressResolution,
        EgressToken, HarnessAdapter, ResourceMapper,
    },
    contract::{RequiredText, RuntimeEpoch, SageAgentSessionId},
    intent::{CapabilitySnapshotV1, ExternalId},
    limits::LIMITS,
    transport::{Frame, OutboundFrame, request_payload_digest},
};

use super::{AdapterError, Operation, Outbound, PiTranslator, ResearchPiBinding, TerminalOutcome};

#[path = "runtime/support.rs"]
mod support;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RequestKind {
    InitializeFirst,
    InitializeSecond,
    Models,
    NewSession,
    Prompt,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeliveryPhase {
    Reserved,
    Queued,
}

struct RequestReservation {
    command_id: String,
    kind: RequestKind,
    phase: DeliveryPhase,
}

pub(super) struct TurnIntent {
    external_id: ExternalId,
    client_message_id: RequiredText<512>,
    text_id: Option<ExternalId>,
    text_snapshot: Option<String>,
    next_item: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DenyStage {
    Prepared,
    Committed,
    Queued,
    Written,
}

pub(super) struct DenyIntent {
    pub(super) request_id: ExternalId,
    pub(super) command_id: String,
    pub(super) stage: DenyStage,
}

pub(crate) struct PiRuntime {
    pub(super) translator: PiTranslator,
    pub(super) binding_digest: [u8; 32],
    pub(super) runtime_epoch: RuntimeEpoch,
    pub(super) session_id: SageAgentSessionId,
    pub(super) external_session: Option<ExternalId>,
    pub(super) active_turn: Option<TurnIntent>,
    pub(super) deny: Option<DenyIntent>,
    staged_initialize: Option<Outbound>,
    reservations: BTreeMap<u64, RequestReservation>,
    next_egress: u64,
    next_session: u64,
    pub(super) model_count: u16,
    pub(super) current_model: Option<String>,
    pub(super) initialized: bool,
    poisoned: bool,
}

impl PiRuntime {
    pub(crate) fn from_binding(
        binding: &ResearchPiBinding,
        binding_digest: [u8; 32],
        runtime_epoch: RuntimeEpoch,
        session_id: SageAgentSessionId,
    ) -> Result<Self, AdapterRuntimeError> {
        let _ = (binding_digest, runtime_epoch, session_id);
        binding
            .require_launch_admission()
            .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        Err(AdapterRuntimeError::LaunchUnavailable)
    }

    #[cfg(test)]
    fn fixture(runtime_epoch: RuntimeEpoch, session_id: SageAgentSessionId) -> Self {
        Self::new(
            PiTranslator::fixture_admitted(runtime_epoch.get()),
            [7; 32],
            runtime_epoch,
            session_id,
        )
    }

    fn new(
        translator: PiTranslator,
        binding_digest: [u8; 32],
        runtime_epoch: RuntimeEpoch,
        session_id: SageAgentSessionId,
    ) -> Self {
        Self {
            translator,
            binding_digest,
            runtime_epoch,
            session_id,
            external_session: None,
            active_turn: None,
            deny: None,
            staged_initialize: None,
            reservations: BTreeMap::new(),
            next_egress: 0,
            next_session: 0,
            model_count: 0,
            current_model: None,
            initialized: false,
            poisoned: false,
        }
    }

    fn begin_one(
        &mut self,
        operation: Operation,
        kind: RequestKind,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        let mut values = self.translator.begin(operation).map_err(support::map_pi)?;
        if values.len() != 1 {
            return Err(AdapterRuntimeError::Operation);
        }
        self.reserve(values.remove(0), kind)
    }

    fn begin_initialize(&mut self) -> Result<AdapterEgress, AdapterRuntimeError> {
        if self.initialized || self.staged_initialize.is_some() {
            return Err(AdapterRuntimeError::Operation);
        }
        let mut values = self
            .translator
            .begin(Operation::Initialize)
            .map_err(support::map_pi)?;
        if values.len() != 2 {
            return Err(AdapterRuntimeError::Operation);
        }
        let second = values.pop().ok_or(AdapterRuntimeError::Operation)?;
        let first = values.pop().ok_or(AdapterRuntimeError::Operation)?;
        let egress = self.reserve(first, RequestKind::InitializeFirst)?;
        self.staged_initialize = Some(second);
        Ok(egress)
    }

    fn reserve(
        &mut self,
        outbound: Outbound,
        kind: RequestKind,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        if self.reservations.len() >= LIMITS.queues.outstanding_rpc_per_session as usize {
            self.translator
                .reject_before_write(&outbound.command_id)
                .map_err(support::map_pi)?;
            return Err(AdapterRuntimeError::BatchCount);
        }
        let frame = match OutboundFrame::new(outbound.line.into_bytes()) {
            Ok(frame) => frame,
            Err(_) => {
                self.translator
                    .reject_before_write(&outbound.command_id)
                    .map_err(support::map_pi)?;
                return Err(AdapterRuntimeError::Frame);
            }
        };
        let next_egress = match self
            .next_egress
            .checked_add(1)
            .filter(|value| *value <= i64::MAX as u64)
        {
            Some(value) => value,
            None => {
                self.translator
                    .reject_before_write(&outbound.command_id)
                    .map_err(support::map_pi)?;
                return Err(AdapterRuntimeError::Operation);
            }
        };
        self.next_egress = next_egress;
        self.reservations.insert(
            self.next_egress,
            RequestReservation {
                command_id: outbound.command_id,
                kind,
                phase: DeliveryPhase::Reserved,
            },
        );
        let token = if kind == RequestKind::Cancel {
            EgressToken::cancel_request(self.next_egress)
        } else {
            EgressToken::request(self.next_egress)
        };
        Ok(AdapterEgress::new(frame, token))
    }

    fn rollback(&mut self, id: u64) -> Result<(), AdapterRuntimeError> {
        let reservation = self
            .reservations
            .remove(&id)
            .ok_or(AdapterRuntimeError::EgressIntent)?;
        self.translator
            .reject_before_write(&reservation.command_id)
            .map_err(support::map_pi)?;
        match reservation.kind {
            RequestKind::InitializeFirst | RequestKind::InitializeSecond => {
                self.staged_initialize = None;
                self.initialized = false;
            }
            RequestKind::Prompt => self.active_turn = None,
            RequestKind::Models | RequestKind::NewSession | RequestKind::Cancel => {}
        }
        Ok(())
    }

    fn snapshot(&self) -> CapabilitySnapshotV1 {
        support::capabilities(self.model_count)
    }
}

impl HarnessAdapter for PiRuntime {
    fn capabilities(&self) -> Option<CapabilitySnapshotV1> {
        self.initialized.then(|| self.snapshot())
    }

    fn command(&mut self, command: AdapterCommand) -> Result<AdapterEgress, AdapterRuntimeError> {
        if self.poisoned {
            return Err(AdapterRuntimeError::Operation);
        }
        match command {
            AdapterCommand::Initialize => self.begin_initialize(),
            AdapterCommand::ModelPage { cursor: None } if self.initialized => {
                self.begin_one(Operation::GetModels, RequestKind::Models)
            }
            AdapterCommand::ModelPage { .. } => Err(AdapterRuntimeError::UnsupportedCommand),
            AdapterCommand::NewSession {
                model: Some(model), ..
            } if self.initialized && self.current_model.as_deref() == Some(model.as_str()) => {
                self.begin_one(Operation::NewSession, RequestKind::NewSession)
            }
            AdapterCommand::NewSession { model: Some(_), .. } => {
                Err(AdapterRuntimeError::UnsupportedCommand)
            }
            AdapterCommand::NewSession { .. } if self.initialized => {
                self.begin_one(Operation::NewSession, RequestKind::NewSession)
            }
            AdapterCommand::NewSession { .. } => Err(AdapterRuntimeError::Operation),
            AdapterCommand::ResumeSession { .. }
            | AdapterCommand::LoadSession { .. }
            | AdapterCommand::CloseSession => Err(AdapterRuntimeError::UnsupportedCommand),
            AdapterCommand::Prompt {
                prompt,
                client_message_id,
            } => {
                if self.external_session.is_none() || self.active_turn.is_some() {
                    return Err(AdapterRuntimeError::Operation);
                }
                let external_id =
                    ExternalId::new("piTurnId", client_message_id.as_str().to_owned())
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
                    text_id: None,
                    text_snapshot: None,
                    next_item: 0,
                });
                Ok(egress)
            }
            AdapterCommand::Cancel if self.active_turn.is_some() => {
                self.begin_one(Operation::Abort, RequestKind::Cancel)
            }
            AdapterCommand::Cancel => Err(AdapterRuntimeError::Operation),
            AdapterCommand::ResolvePermission {
                request_id,
                decision,
            } => self.resolve_later_permission(&request_id, decision),
        }
    }

    fn ingest(
        &mut self,
        frame: &Frame,
        _resources: &dyn ResourceMapper,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        if self.poisoned {
            return Err(AdapterRuntimeError::Operation);
        }
        let outputs = match self.translator.ingest(frame.as_bytes()) {
            Ok(outputs) => outputs,
            Err(error) => {
                if self.staged_initialize.take().is_some() {
                    self.poisoned = true;
                }
                return Err(support::map_pi(error));
            }
        };
        let hidden_models = self.staged_initialize.is_some()
            && outputs
                .iter()
                .any(|value| matches!(value, super::AdapterOutput::Models(_)));
        if hidden_models {
            self.staged_initialize = None;
            self.poisoned = true;
            return Err(AdapterRuntimeError::Frame);
        }
        let advance = self.staged_initialize.is_some()
            && outputs
                .iter()
                .any(|value| matches!(value, super::AdapterOutput::State(_)));
        let failed = outputs
            .iter()
            .any(|value| matches!(value, super::AdapterOutput::Error { .. }));
        if failed && self.staged_initialize.is_some() {
            self.staged_initialize = None;
        }
        let staged = if advance {
            let outbound = self
                .staged_initialize
                .take()
                .ok_or(AdapterRuntimeError::Operation)?;
            Some(self.reserve(outbound, RequestKind::InitializeSecond)?)
        } else {
            None
        };
        let request_digest =
            request_payload_digest(frame).map_err(|_| AdapterRuntimeError::Frame)?;
        self.translate_outputs(outputs, staged, request_digest)
    }

    fn commit_mandatory_egress(&mut self, token: &EgressToken) -> Result<(), AdapterRuntimeError> {
        self.commit_deny(token)
    }

    fn resolve_egress(
        &mut self,
        token: EgressToken,
        resolution: EgressResolution,
    ) -> Result<(), AdapterRuntimeError> {
        if token.deny_request().is_some() {
            return self.resolve_deny(token, resolution);
        }
        if token.allow_request().is_some() {
            return match resolution {
                EgressResolution::Queued => Ok(()),
                EgressResolution::Written => {
                    self.deny = None;
                    Ok(())
                }
                EgressResolution::QueueRejected
                | EgressResolution::WriteZeroBytes
                | EgressResolution::DeliveryUnknown => Err(AdapterRuntimeError::EgressIntent),
            };
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
                self.staged_initialize = None;
                self.poisoned = true;
                Ok(())
            }
        }
    }
}

#[cfg(test)]
#[path = "runtime/tests.rs"]
mod tests;
