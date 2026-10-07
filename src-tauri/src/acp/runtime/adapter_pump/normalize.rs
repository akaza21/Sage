use super::{
    coalescer::NormalizedFact,
    payload::{payload_json, payload_measure},
    *,
};
use sha2::{Digest, Sha256};

impl<A: HarnessAdapter, J: AdapterBatchJournal, R: ResourceMapper> AdapterPump<A, J, R> {
    pub(super) fn normalize(
        &mut self,
        event: AdapterEvent,
        now_millis: i64,
        terminal: Option<TurnState>,
    ) -> Result<Option<NormalizedFact>, AdapterPumpError> {
        match event {
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
            } if self.exact_turn(&turn_id) && self.exact_turn(&item_id) && terminal.is_none() => {
                let request_id = self.scoped_permission_id(&external_request_id)?;
                let (normalized_payload_json, measured) = payload_json(&payload)?;
                Ok(Some(NormalizedFact::new(
                    DurableAdapterEvent::PermissionRequest {
                        intent: PermissionIntent {
                            request_id,
                            project_id: self.intent.project_id.clone(),
                            chat_id: self.intent.chat_id.clone(),
                            session_id: self.intent.session_id.clone(),
                            turn_id: self.intent.active_turn.turn_id.clone(),
                            runtime_epoch: self.intent.runtime_epoch,
                            binding_digest: hex(&self.intent.binding_digest),
                            external_request_id,
                            kind: RequiredText::new("requestKind", "permission")
                                .map_err(|_| AdapterPumpError::Intent)?,
                            payload_digest: hex(&request_payload_digest),
                            normalized_payload_digest: measured.digest,
                            normalized_payload_json,
                        },
                        now_millis,
                    },
                    measured.bytes,
                )))
            }
            AdapterEvent::PermissionResolved {
                turn_id,
                external_request_id,
                state,
            } if self.exact_turn(&turn_id) && state.is_terminal() => {
                let request_id = self.scoped_permission_id(&external_request_id)?;
                Ok(Some(NormalizedFact::new(
                    DurableAdapterEvent::PermissionResolved {
                        scope: self.scope(),
                        request_id,
                        state,
                        now_millis,
                    },
                    external_request_id.as_str().len(),
                )))
            }
            AdapterEvent::Error(payload) | AdapterEvent::Unsupported(payload)
                if terminal.is_none() =>
            {
                self.runtime_payload(payload, now_millis, None)
            }
            AdapterEvent::SessionReady { id, .. } if !self.exact_session(&id) => Ok(None),
            AdapterEvent::Turn { .. }
            | AdapterEvent::Item { .. }
            | AdapterEvent::FilePair { .. }
            | AdapterEvent::ActivityItem(_)
            | AdapterEvent::Usage { .. }
            | AdapterEvent::PermissionRequest { .. }
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

    fn scoped_permission_id(
        &self,
        external_request_id: &ExternalId,
    ) -> Result<SageServerRequestId, AdapterPumpError> {
        let session_digest = Sha256::digest(self.intent.session_id.as_str().as_bytes());
        let external_digest = Sha256::digest(external_request_id.as_str().as_bytes());
        SageServerRequestId::parse(format!(
            "permission:{session_digest:x}:{}:{external_digest:x}",
            self.intent.runtime_epoch.get()
        ))
        .map_err(|_| AdapterPumpError::Intent)
    }

    pub(super) fn latch_frame<T>(&mut self) -> Result<T, AdapterPumpError> {
        self.pending.clear();
        self.teardown = true;
        Err(AdapterPumpError::Frame)
    }
}

pub(super) fn control_critical(fact: &NormalizedFact) -> bool {
    match &fact.event {
        DurableAdapterEvent::PermissionRequest { .. } => true,
        DurableAdapterEvent::PermissionDeny { .. } => true,
        DurableAdapterEvent::PermissionResolved { .. } => true,
        DurableAdapterEvent::Turn { state, .. } => state.is_terminal(),
        DurableAdapterEvent::RuntimePayload { payload, .. } => matches!(
            payload,
            NormalizedPayload::Error { .. } | NormalizedPayload::Unsupported { .. }
        ),
        DurableAdapterEvent::UsageSnapshot { .. } => false,
        DurableAdapterEvent::TerminalReason { .. } => true,
        DurableAdapterEvent::Item { .. } => false,
        DurableAdapterEvent::FilePair { .. } => false,
        DurableAdapterEvent::ActivityItem(_) => false,
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
