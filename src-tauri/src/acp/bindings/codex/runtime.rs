use crate::acp::{
    bindings::adapter::{
        AdapterBatch, AdapterBatchBuilder, AdapterCommand, AdapterEgress, AdapterEvent,
        AdapterRuntimeError, EgressResolution, EgressToken, HarnessAdapter, ResourceMapper,
    },
    contract::{RuntimeEpoch, SageAgentSessionId},
    lifecycle::ServerRequestState,
    transport::{Frame, OutboundFrame, request_payload_digest},
};

use super::{
    incoming::{IncomingEvent, IngressDisposition},
    manifest::MetadataValidatedPacket,
    policy::SecurityPolicyPrerequisite,
    request::RequestKind,
    state::CodexAdapter,
};

#[cfg(test)]
use super::manifest::{Platform, parse_packet_metadata};

#[path = "runtime/support.rs"]
mod support;
use support::*;

#[path = "runtime/snapshots.rs"]
mod snapshots;
use snapshots::ItemSnapshots;

pub(crate) struct CodexRuntime {
    inner: CodexAdapter,
    next_request_id: u64,
    snapshots: ItemSnapshots,
    poisoned: bool,
}

impl CodexRuntime {
    pub(crate) fn from_metadata_packet(
        packet: &MetadataValidatedPacket,
        security_policy: &SecurityPolicyPrerequisite,
        binding_digest: [u8; 32],
        runtime_epoch: RuntimeEpoch,
        session_id: SageAgentSessionId,
    ) -> Result<Self, AdapterRuntimeError> {
        security_policy
            .revalidate(packet)
            .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        packet
            .require_physical_admission()
            .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        Ok(Self::new(packet, binding_digest, runtime_epoch, session_id))
    }

    #[cfg(test)]
    pub(super) fn fixture(platform: Platform) -> Self {
        assert_eq!(platform, Platform::WindowsX86_64);
        let packet = parse_packet_metadata(include_bytes!(
            "../../../../testdata/acp/codex/candidate-windows.json"
        ))
        .expect("pinned Codex fixture");
        Self::new(
            &packet,
            [9; 32],
            RuntimeEpoch::new(1).expect("fixture epoch"),
            SageAgentSessionId::parse("session-codex-runtime").expect("fixture session"),
        )
    }

    fn new(
        packet: &MetadataValidatedPacket,
        binding_digest: [u8; 32],
        runtime_epoch: RuntimeEpoch,
        session_id: SageAgentSessionId,
    ) -> Self {
        Self {
            inner: CodexAdapter::from_metadata_packet(
                packet,
                binding_digest,
                runtime_epoch,
                session_id,
            ),
            next_request_id: 0,
            snapshots: ItemSnapshots::new(),
            poisoned: false,
        }
    }

    fn translate(
        &mut self,
        disposition: IngressDisposition,
        request_digest: [u8; 32],
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        match disposition {
            IngressDisposition::Events(events) => {
                if events.len()
                    > crate::acp::limits::LIMITS
                        .queues
                        .journal_facts_per_transaction as usize
                {
                    return Err(AdapterRuntimeError::BatchCount);
                }
                let mut batch = AdapterBatchBuilder::new();
                for event in events {
                    let (event, egress) = self.translate_event(event, request_digest)?;
                    let mandatory = egress
                        .as_ref()
                        .is_some_and(|value| value.token().is_mandatory());
                    batch.push_event(event).map_err(|error| {
                        if mandatory {
                            AdapterRuntimeError::MandatoryDenyTeardown
                        } else {
                            error
                        }
                    })?;
                    if let Some(egress) = egress {
                        batch.push_egress(egress).map_err(|error| {
                            if mandatory {
                                AdapterRuntimeError::MandatoryDenyTeardown
                            } else {
                                error
                            }
                        })?;
                    }
                }
                Ok(batch.finish())
            }
            IngressDisposition::IgnoredLate => one(AdapterEvent::IgnoredLate),
            IngressDisposition::DuplicateTerminal => one(AdapterEvent::IgnoredDuplicate),
            IngressDisposition::DuplicateServerRequest { denial: None } => {
                one(AdapterEvent::IgnoredDuplicate)
            }
            IngressDisposition::DuplicateServerRequest {
                denial: Some(bytes),
            } => {
                let request_id = denial_request_id(&bytes)?;
                let frame = OutboundFrame::new(bytes).map_err(|_| AdapterRuntimeError::Frame)?;
                AdapterBatch::new(
                    vec![AdapterEvent::IgnoredDuplicate],
                    vec![AdapterEgress::new(
                        frame,
                        EgressToken::mandatory_deny(request_id, true),
                    )],
                )
                .map_err(|_| AdapterRuntimeError::MandatoryDenyTeardown)
            }
        }
    }

    fn translate_event(
        &mut self,
        event: IncomingEvent,
        request_digest: [u8; 32],
    ) -> Result<(AdapterEvent, Option<AdapterEgress>), AdapterRuntimeError> {
        let (event, egress) = match event {
            IncomingEvent::Initialized { capabilities } => {
                let frame = OutboundFrame::new(self.inner.initialized().map_err(map_adapter)?)
                    .map_err(|_| AdapterRuntimeError::Frame)?;
                (
                    AdapterEvent::Initialized { capabilities },
                    Some(AdapterEgress::new(
                        frame,
                        EgressToken::protocol_notification(),
                    )),
                )
            }
            IncomingEvent::SessionReady { id } => (
                {
                    self.snapshots.clear();
                    AdapterEvent::SessionReady {
                        id: identity(id),
                        loaded: false,
                    }
                },
                None,
            ),
            IncomingEvent::SessionLoaded { id } => (
                {
                    self.snapshots.clear();
                    AdapterEvent::SessionReady {
                        id: identity(id),
                        loaded: true,
                    }
                },
                None,
            ),
            IncomingEvent::Models {
                models,
                next_cursor,
            } => (model_event(models, next_cursor)?, None),
            IncomingEvent::Turn { id, state } => (
                {
                    if state.is_terminal() {
                        self.snapshots.clear();
                    }
                    AdapterEvent::Turn {
                        id: identity(id),
                        state,
                    }
                },
                None,
            ),
            IncomingEvent::Item {
                id,
                lifecycle,
                tool_state,
                payload,
                update_mode,
            } => (
                AdapterEvent::Item {
                    id: identity(id.clone()),
                    lifecycle,
                    tool_state,
                    payload: match self.snapshots.apply(&id, lifecycle, update_mode, payload) {
                        Ok(payload) => payload,
                        Err(error) => {
                            self.poisoned = true;
                            self.snapshots.clear();
                            return Err(error);
                        }
                    },
                },
                None,
            ),
            IncomingEvent::Permission {
                turn_id,
                item_id,
                payload,
                denial,
            } => {
                let request_id = denial.external_request.external_id.clone();
                let frame = OutboundFrame::new(denial.bytes)
                    .map_err(|_| AdapterRuntimeError::MandatoryDenyTeardown)?;
                self.inner
                    .advance_permission_decline(
                        request_id.as_str(),
                        ServerRequestState::DeclinePending,
                    )
                    .map_err(|_| AdapterRuntimeError::MandatoryDenyTeardown)?;
                (
                    AdapterEvent::PermissionRequest {
                        turn_id: identity(turn_id),
                        item_id: identity(item_id),
                        payload,
                        request_payload_digest: request_digest,
                    },
                    Some(AdapterEgress::new(
                        frame,
                        EgressToken::mandatory_deny(request_id, false),
                    )),
                )
            }
            IncomingEvent::Usage { turn_id, payload } => (
                AdapterEvent::Usage {
                    turn_id: identity(turn_id),
                    payload,
                },
                None,
            ),
            IncomingEvent::Acknowledged { closes_session } => {
                if closes_session {
                    self.snapshots.clear();
                }
                (AdapterEvent::Acknowledged, None)
            }
            IncomingEvent::Unsupported(payload) => (AdapterEvent::Unsupported(payload), None),
            IncomingEvent::Error(payload) => (AdapterEvent::Error(payload), None),
        };
        Ok((event, egress))
    }
}

impl HarnessAdapter for CodexRuntime {
    fn capabilities(&self) -> Option<crate::acp::intent::CapabilitySnapshotV1> {
        self.inner.effective_capabilities().ok()
    }

    fn command(&mut self, command: AdapterCommand) -> Result<AdapterEgress, AdapterRuntimeError> {
        if self.poisoned {
            return Err(AdapterRuntimeError::Operation);
        }
        match command {
            AdapterCommand::Initialize => self.request(CodexAdapter::initialize),
            AdapterCommand::ModelPage { cursor } => {
                self.request(|adapter, id| adapter.models(id, cursor.as_ref()))
            }
            AdapterCommand::NewSession { native_root, model } => self.request(|adapter, id| {
                let root = adapter.project_root(native_root)?;
                adapter.start(id, &root, model.as_ref())
            }),
            AdapterCommand::ResumeSession {
                external_session_id,
                native_root,
                model,
            } => self.request(|adapter, id| {
                let root = adapter.project_root(native_root)?;
                adapter.resume(id, &external_session_id, &root, model.as_ref())
            }),
            AdapterCommand::LoadSession {
                external_session_id,
            } => self.request(|adapter, id| adapter.read(id, &external_session_id)),
            AdapterCommand::CloseSession => self.request(CodexAdapter::close),
            AdapterCommand::Prompt {
                prompt,
                client_message_id,
            } => self.request(|adapter, id| adapter.prompt(id, &prompt, &client_message_id)),
            AdapterCommand::Cancel => self.request_with_control(CodexAdapter::cancel, true),
        }
    }

    fn ingest(
        &mut self,
        frame: &Frame,
        resources: &dyn ResourceMapper,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        if self.poisoned {
            return Err(AdapterRuntimeError::Operation);
        }
        let mapper = MapperBridge(resources);
        let disposition = self
            .inner
            .ingest(frame.as_bytes(), &mapper)
            .map_err(map_adapter)?;
        let request_digest =
            request_payload_digest(frame).map_err(|_| AdapterRuntimeError::Frame)?;
        self.translate(disposition, request_digest)
    }

    fn commit_mandatory_egress(&mut self, token: &EgressToken) -> Result<(), AdapterRuntimeError> {
        let Some((request_id, replay)) = token.deny_request() else {
            return Err(AdapterRuntimeError::EgressIntent);
        };
        if replay {
            return Ok(());
        }
        self.inner
            .advance_permission_decline(request_id, ServerRequestState::DeclineCommitted)
            .map_err(|_| AdapterRuntimeError::MandatoryDenyTeardown)
    }

    fn resolve_egress(
        &mut self,
        token: EgressToken,
        resolution: EgressResolution,
    ) -> Result<(), AdapterRuntimeError> {
        if let Some(request_id) = token.request_id() {
            if matches!(
                resolution,
                EgressResolution::QueueRejected | EgressResolution::WriteZeroBytes
            ) {
                let clear = self.inner.pending_kind(request_id) == Some(RequestKind::TurnStart);
                self.inner
                    .reject_before_write(request_id)
                    .map_err(map_adapter)?;
                if clear {
                    self.snapshots.clear();
                }
            }
            return Ok(());
        }
        if token.is_notification() {
            return match resolution {
                EgressResolution::QueueRejected
                | EgressResolution::WriteZeroBytes
                | EgressResolution::DeliveryUnknown => Err(AdapterRuntimeError::EgressIntent),
                EgressResolution::Queued | EgressResolution::Written => Ok(()),
            };
        }
        let Some((request_id, replay)) = token.deny_request() else {
            return Err(AdapterRuntimeError::EgressIntent);
        };
        if matches!(
            resolution,
            EgressResolution::QueueRejected
                | EgressResolution::WriteZeroBytes
                | EgressResolution::DeliveryUnknown
        ) {
            return Err(AdapterRuntimeError::MandatoryDenyTeardown);
        }
        if replay {
            return Ok(());
        }
        let next = match resolution {
            EgressResolution::Queued => ServerRequestState::ResponseQueued,
            EgressResolution::Written => ServerRequestState::ResolvedDeclined,
            EgressResolution::QueueRejected
            | EgressResolution::WriteZeroBytes
            | EgressResolution::DeliveryUnknown => unreachable!(),
        };
        self.inner
            .advance_permission_decline(request_id, next)
            .map_err(|_| AdapterRuntimeError::MandatoryDenyTeardown)
    }
}

#[cfg(test)]
#[path = "runtime/tests.rs"]
mod tests;
