use crate::acp::{
    bindings::adapter::{
        AdapterBatch, AdapterBatchBuilder, AdapterEgress, AdapterEvent, AdapterRuntimeError,
        EgressToken,
    },
    declared_test::DeclaredTestCommands,
    transport::OutboundFrame,
};

use super::{
    incoming::{IncomingEvent, IngressDisposition},
    state::CodexAdapter,
};

mod support;
use support::*;

mod admission;

pub(super) mod authority;
use authority::RuntimeLaunchAuthority;

mod launch;

mod snapshots;
use snapshots::ItemSnapshots;

mod harness;

mod factory;
pub(crate) use factory::CodexRuntimeFactory;

pub(crate) struct CodexRuntime {
    launch_authority: RuntimeLaunchAuthority,
    mcp: crate::acp::mcp::McpAdmission,
    declared_test_commands: DeclaredTestCommands,
    inner: CodexAdapter,
    next_request_id: u64,
    snapshots: ItemSnapshots,
    poisoned: bool,
    pending_browser_failure_details: Vec<(String, String, usize)>,
    browser_failure_warning_logged: bool,
}

impl CodexRuntime {
    #[cfg(test)]
    pub(crate) fn enable_observed_goal_for_test(&mut self) {
        self.inner.enable_observed_goal_for_test();
        self.inner.goal_updated(&serde_json::json!({
            "objective":"Continue bounded work","status":"active",
            "tokensUsed":0,"timeUsedSeconds":0
        }));
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
                    if let IncomingEvent::HostToolReply { bytes } = event {
                        let frame =
                            OutboundFrame::new(bytes).map_err(|_| AdapterRuntimeError::Frame)?;
                        batch
                            .push_egress(AdapterEgress::new(
                                frame,
                                EgressToken::protocol_notification(),
                            ))
                            .map_err(|_| AdapterRuntimeError::BatchCount)?;
                        continue;
                    }
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
            IncomingEvent::HostToolReply { .. } => unreachable!("handled before translation"),
            IncomingEvent::PermissionResolved {
                turn_id,
                external_request,
                state,
            } => (
                AdapterEvent::PermissionResolved {
                    turn_id: identity(turn_id),
                    external_request_id: external_request.external_id,
                    state,
                },
                None,
            ),
            IncomingEvent::Acknowledged { closes_session } => {
                if closes_session {
                    self.snapshots.clear();
                }
                (AdapterEvent::Acknowledged, None)
            }
            IncomingEvent::SteerResponse {
                request_id,
                accepted,
            } => (
                AdapterEvent::SteerResponse {
                    request_id,
                    accepted,
                },
                None,
            ),
            IncomingEvent::Unsupported(payload) => (AdapterEvent::Unsupported(payload), None),
            IncomingEvent::Error(payload) => (AdapterEvent::Error(payload), None),
        };
        Ok((event, egress))
    }
}

#[cfg(test)]
mod tests;
