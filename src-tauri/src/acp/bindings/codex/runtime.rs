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
mod ingress;
mod model_config;

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
    native_rpc_reply: Option<crate::acp::goal::NativeRpcReply>,
    selected_model_id: Option<crate::acp::intent::ModelId>,
    reported_model_id: Option<crate::acp::intent::ModelId>,
    selected_model: Option<super::incoming::ModelDescriptor>,
    selected_effort: Option<crate::acp::contract::RequiredText<512>>,
    model_list_requested: bool,
    model_catalog_idle_only: bool,
    model_catalog_entries: usize,
    model_catalog_bytes: usize,
    next_model_cursor: Option<crate::acp::intent::PageCursor>,
    model_pages_seen: u64,
    session_config: Option<crate::acp::session_config::SessionConfig>,
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

    #[cfg(test)]
    pub(crate) fn learn_child_for_test(&mut self, child: &str) {
        self.inner.learn_child_for_test(child);
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
                    // A notice needs the Turn's durable scope, so emit it only
                    // after the first running Turn has been bound.
                    let browser_notice = if matches!(
                        &event,
                        IncomingEvent::Turn {
                            state: crate::acp::lifecycle::TurnState::Running,
                            ..
                        }
                    ) {
                        self.mcp.take_browser_unavailable_notice()
                    } else {
                        None
                    };
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
                    if let Some(message) = browser_notice {
                        batch.push_event(AdapterEvent::Error(
                            crate::acp::intent::NormalizedPayload::Error {
                                kind: None,
                                code: crate::acp::contract::SafeText::new(
                                    "code",
                                    "browserMcpUnavailable",
                                )
                                .map_err(|_| AdapterRuntimeError::Frame)?,
                                message: crate::acp::contract::SafeText::new("message", message)
                                    .map_err(|_| AdapterRuntimeError::Frame)?,
                                retryable: false,
                            },
                        ))?;
                    }
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
            IngressDisposition::DroppedRawReasoning => one(AdapterEvent::Absorbed),
            IngressDisposition::GoalMutationRejected => {
                Err(AdapterRuntimeError::GoalMutationRejected)
            }
            IngressDisposition::GoalMutationApplied => {
                self.native_rpc_reply = Some(crate::acp::goal::NativeRpcReply::GoalApplied);
                one(AdapterEvent::IgnoredLate)
            }
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
            IncomingEvent::SessionReady {
                id,
                current_model,
                current_effort,
            } => (
                {
                    self.snapshots.clear();
                    self.reported_model_id = current_model.clone();
                    if let Some(model) = current_model {
                        if self.selected_model_id.as_ref() != Some(&model) {
                            self.selected_model = None;
                            self.session_config = None;
                        }
                        self.selected_model_id = Some(model);
                    }
                    if current_effort.is_some() {
                        self.selected_effort = current_effort;
                    }
                    self.refresh_model_config();
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
            } => self.apply_model_page(models, next_cursor),
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
            IncomingEvent::ChildThreadItem {
                id,
                lifecycle,
                payload,
                child_thread_id,
            } => (
                AdapterEvent::ChildThreadItem {
                    id: identity(id),
                    lifecycle,
                    payload,
                    child_thread_id,
                },
                None,
            ),
            IncomingEvent::ChildLiveItem {
                child_thread_id,
                child_turn_id,
                item_id,
                role,
                lifecycle,
                tool_state,
                payload,
            } => (
                AdapterEvent::ChildLiveItem {
                    child_thread_id,
                    child_turn_id,
                    item_id,
                    role,
                    lifecycle,
                    tool_state,
                    payload,
                },
                None,
            ),
            IncomingEvent::ChildTurnStatus {
                child_thread_id,
                child_turn_id,
                status,
            } => (
                AdapterEvent::ChildTurnStatus {
                    child_thread_id,
                    child_turn_id,
                    status,
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
