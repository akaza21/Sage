use crate::acp::{
    bindings::adapter::{
        AdapterBatch, AdapterBatchBuilder, AdapterEgress, AdapterEvent, AdapterRuntimeError,
        EgressToken,
    },
    transport::OutboundFrame,
};

use super::{
    incoming::{IncomingEvent, IngressDisposition},
    state::CodexAdapter,
};

#[path = "runtime/support.rs"]
mod support;
use support::*;

#[path = "runtime/admission.rs"]
mod admission;

#[path = "runtime/authority.rs"]
pub(super) mod authority;
use authority::RuntimeLaunchAuthority;

#[path = "runtime/launch.rs"]
mod launch;

#[path = "runtime/snapshots.rs"]
mod snapshots;
use snapshots::ItemSnapshots;

#[path = "runtime/harness.rs"]
mod harness;

pub(crate) struct CodexRuntime {
    launch_authority: RuntimeLaunchAuthority,
    inner: CodexAdapter,
    next_request_id: u64,
    snapshots: ItemSnapshots,
    poisoned: bool,
}

impl CodexRuntime {
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
            } => (
                AdapterEvent::PermissionRequest {
                    turn_id: identity(turn_id),
                    item_id: identity(item_id),
                    external_request_id: denial.external_request.external_id,
                    payload,
                    request_payload_digest: request_digest,
                },
                None,
            ),
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

#[cfg(test)]
#[path = "runtime/tests.rs"]
mod tests;
