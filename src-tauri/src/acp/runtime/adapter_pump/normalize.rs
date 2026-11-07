use super::{
    coalescer::NormalizedFact,
    payload::{payload_json, payload_measure},
    *,
};

mod critical;
mod permission;
mod terminal_reason;
pub(super) use critical::control_critical;
use permission::PermissionRequestEvent;

impl<A: HarnessAdapter, J: AdapterBatchJournal, R: ResourceMapper> AdapterPump<A, J, R> {
    fn synthetic_item_id(&mut self) -> Result<ExternalId, AdapterPumpError> {
        self.next_synthetic_item = self
            .next_synthetic_item
            .checked_add(1)
            .filter(|value| *value <= i64::MAX as u64)
            .ok_or(AdapterPumpError::BatchCapacity)?;
        ExternalId::new(
            "runtimeItemId",
            format!(
                "sage-runtime-{}-{}",
                self.intent.runtime_epoch.get(),
                self.next_synthetic_item
            ),
        )
        .map_err(|_| AdapterPumpError::BatchCapacity)
    }

    pub(super) fn normalize(
        &mut self,
        event: AdapterEvent,
        now_millis: i64,
        terminal: Option<TurnState>,
    ) -> Result<Option<NormalizedFact>, AdapterPumpError> {
        match event {
            AdapterEvent::TerminalReason {
                id,
                reason,
                message,
            } if self.exact_turn(&id) && terminal.is_none() => Ok(Some(
                terminal_reason::normalize(self.scope(), reason, message, now_millis),
            )),
            AdapterEvent::Turn { id, state } if self.exact_turn(&id) => {
                if terminal.is_some() {
                    return Ok(None);
                }
                Ok(Some(NormalizedFact::new(
                    DurableAdapterEvent::Turn {
                        scope: self.scope(),
                        state,
                        now_millis,
                    },
                    0,
                )))
            }
            AdapterEvent::Item {
                id,
                lifecycle,
                tool_state,
                payload,
            } if self.exact_turn(&id) && terminal.is_none() => {
                let measured = payload_measure(&payload)?;
                Ok(Some(NormalizedFact::new(
                    DurableAdapterEvent::Item {
                        scope: self.scope(),
                        external_item_id: id.external_id,
                        lifecycle,
                        tool_state,
                        payload,
                        payload_digest: measured.digest,
                        now_millis,
                    },
                    measured.bytes,
                )))
            }
            AdapterEvent::ChildThreadItem {
                id,
                lifecycle,
                payload,
                child_thread_id,
            } if self.exact_turn(&id) && terminal.is_none() => {
                if !matches!(payload, NormalizedPayload::Collaboration { .. })
                    || child_thread_id == self.intent.external_session_id
                {
                    return Err(AdapterPumpError::Intent);
                }
                let measured = payload_measure(&payload)?;
                Ok(Some(NormalizedFact::new(
                    DurableAdapterEvent::ChildThreadItem {
                        scope: self.scope(),
                        external_item_id: id.external_id,
                        lifecycle,
                        payload,
                        payload_digest: measured.digest,
                        child_thread_id,
                        now_millis,
                    },
                    measured.bytes,
                )))
            }
            AdapterEvent::ChildLiveItem {
                child_thread_id,
                child_turn_id,
                item_id,
                role,
                lifecycle,
                tool_state,
                payload,
            } => {
                let measured = payload_measure(&payload)?;
                Ok(Some(NormalizedFact::new(
                    DurableAdapterEvent::ChildLiveItem {
                        scope: self.scope(),
                        child_thread_id,
                        child_turn_id,
                        item_id,
                        role,
                        lifecycle,
                        tool_state,
                        payload,
                        payload_digest: measured.digest,
                        now_millis,
                    },
                    measured.bytes,
                )))
            }
            AdapterEvent::ChildTurnStatus {
                child_thread_id,
                child_turn_id,
                status,
            } => Ok(Some(NormalizedFact::new(
                DurableAdapterEvent::ChildTurnStatus {
                    scope: self.scope(),
                    child_thread_id,
                    child_turn_id,
                    status,
                    now_millis,
                },
                64,
            ))),
            AdapterEvent::FilePair {
                id,
                external_item_id,
                old_text,
                new_text,
            } if self.exact_turn(&id) && terminal.is_none() => {
                let bytes = old_text
                    .len()
                    .checked_add(new_text.len())
                    .ok_or(AdapterPumpError::BatchCapacity)?;
                if bytes > LIMITS.queues.journal_bytes_per_transaction as usize {
                    return Err(AdapterPumpError::BatchCapacity);
                }
                Ok(Some(NormalizedFact::new(
                    DurableAdapterEvent::FilePair {
                        scope: self.scope(),
                        external_item_id,
                        old_text,
                        new_text,
                        now_millis,
                    },
                    bytes,
                )))
            }
            AdapterEvent::ActivityItem(activity)
                if self.exact_turn(&activity.activity_id)
                    && self.exact_turn(&activity.item_id)
                    && self.exact_turn(&activity.parent_item_id)
                    && activity
                        .parent_activity_id
                        .as_ref()
                        .is_none_or(|parent| self.exact_turn(parent))
                    && terminal.is_none() =>
            {
                let crate::acp::bindings::adapter::AdapterActivityItem {
                    activity_id,
                    item_id,
                    parent_activity_id,
                    parent_item_id,
                    activity_kind,
                    role,
                    activity_lifecycle,
                    depth,
                    observed_sequence,
                    provenance,
                    item_lifecycle,
                    item_tool_state,
                    payload,
                } = *activity;
                let measured = payload_measure(&payload)?;
                Ok(Some(NormalizedFact::new(
                    DurableAdapterEvent::ActivityItem(Box::new(DurableActivityItem {
                        scope: self.scope(),
                        external_activity_id: activity_id.external_id,
                        external_item_id: item_id.external_id,
                        parent_external_activity_id: parent_activity_id
                            .map(|parent| parent.external_id),
                        parent_external_item_id: parent_item_id.external_id,
                        activity_kind,
                        role,
                        activity_lifecycle,
                        depth,
                        observed_sequence,
                        provenance,
                        item_lifecycle,
                        item_tool_state,
                        payload,
                        payload_digest: measured.digest,
                        now_millis,
                    })),
                    measured.bytes,
                )))
            }
            AdapterEvent::Usage { turn_id, payload } if self.exact_turn(&turn_id) => {
                let NormalizedPayload::Usage {
                    input_tokens,
                    output_tokens,
                    context_window,
                } = payload
                else {
                    return Err(AdapterPumpError::Intent);
                };
                let measured = payload_measure(&NormalizedPayload::Usage {
                    input_tokens,
                    output_tokens,
                    context_window,
                })?;
                Ok(Some(NormalizedFact::new(
                    DurableAdapterEvent::UsageSnapshot {
                        scope: self.scope(),
                        input_tokens,
                        output_tokens,
                        context_window,
                        now_millis,
                    },
                    measured.bytes,
                )))
            }
            AdapterEvent::PermissionRequest {
                turn_id,
                item_id,
                external_request_id,
                payload,
                request_payload_digest,
            } => self.permission_request(
                PermissionRequestEvent {
                    turn_id,
                    item_id,
                    external_request_id,
                    payload,
                    request_payload_digest,
                },
                terminal,
                now_millis,
            ),
            AdapterEvent::PermissionResolved {
                turn_id,
                external_request_id,
                state,
            } if self.exact_turn(&turn_id) && state.is_terminal() => {
                self.permission_resolved(external_request_id, state, now_millis)
            }
            AdapterEvent::Error(payload) | AdapterEvent::Unsupported(payload)
                if terminal.is_none() =>
            {
                self.runtime_payload(payload, now_millis, None)
            }
            AdapterEvent::SessionReady { id, .. } if !self.exact_session(&id) => Ok(None),
            AdapterEvent::Turn { .. }
            | AdapterEvent::TerminalReason { .. }
            | AdapterEvent::Item { .. }
            | AdapterEvent::ChildThreadItem { .. }
            | AdapterEvent::FilePair { .. }
            | AdapterEvent::ActivityItem(_)
            | AdapterEvent::Usage { .. }
            | AdapterEvent::PermissionResolved { .. }
            | AdapterEvent::Error(_)
            | AdapterEvent::Unsupported(_) => Ok(None),
            AdapterEvent::IgnoredLate | AdapterEvent::IgnoredDuplicate | AdapterEvent::Absorbed => {
                Ok(None)
            }
            AdapterEvent::Initialized { .. }
            | AdapterEvent::SessionReady { .. }
            | AdapterEvent::Models { .. }
            | AdapterEvent::Acknowledged
            | AdapterEvent::SteerResponse { .. } => Ok(None),
        }
    }

    fn runtime_payload(
        &mut self,
        payload: NormalizedPayload,
        now_millis: i64,
        external_item_id: Option<ExternalId>,
    ) -> Result<Option<NormalizedFact>, AdapterPumpError> {
        let measured = payload_measure(&payload)?;
        Ok(Some(NormalizedFact::new(
            DurableAdapterEvent::RuntimePayload {
                scope: self.scope(),
                external_item_id: match external_item_id {
                    Some(value) => value,
                    None => self.synthetic_item_id()?,
                },
                payload,
                payload_digest: measured.digest,
                now_millis,
            },
            measured.bytes,
        )))
    }

    pub(super) fn latch_frame<T>(&mut self) -> Result<T, AdapterPumpError> {
        self.pending.clear();
        self.teardown = true;
        Err(AdapterPumpError::Frame)
    }
}

pub(super) fn validate_batch(
    events: &[AdapterEvent],
    egress: &[AdapterEgress],
    actual_bytes: usize,
) -> Result<(), AdapterPumpError> {
    if events.len() > LIMITS.queues.journal_facts_per_transaction as usize
        || egress.len() > LIMITS.queues.egress_control_frames_inside_total as usize
    {
        return Err(AdapterPumpError::BatchCapacity);
    }
    let bytes = egress
        .iter()
        .try_fold(0_usize, |sum, value| {
            sum.checked_add(value.frame().wire_len())
        })
        .ok_or(AdapterPumpError::BatchCapacity)?;
    let maximum = (LIMITS.queues.journal_bytes_per_transaction as usize)
        .checked_add(LIMITS.queues.egress_control_bytes_inside_total as usize)
        .ok_or(AdapterPumpError::BatchCapacity)?;
    if bytes > LIMITS.queues.egress_control_bytes_inside_total as usize
        || actual_bytes < bytes
        || actual_bytes > maximum
    {
        return Err(AdapterPumpError::BatchCapacity);
    }
    Ok(())
}

pub(super) fn validate_deny_pairs(
    durable: &[NormalizedFact],
    egress: &[AdapterEgress],
) -> Result<(), AdapterPumpError> {
    for fact in durable {
        let DurableAdapterEvent::PermissionDeny { intent, .. } = &fact.event else {
            continue;
        };
        let matches = egress
            .iter()
            .filter(|value| {
                value.token().deny_request() == Some((intent.external_request_id.as_str(), false))
            })
            .count();
        if matches != 1 {
            return Err(AdapterPumpError::TeardownRequired);
        }
    }
    Ok(())
}
