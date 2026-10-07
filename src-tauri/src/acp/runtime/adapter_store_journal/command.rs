use crate::acp::{
    intent::NormalizedPayload,
    limits::LIMITS,
    runtime::adapter_pump::{DurableAdapterEvent, PumpIntent, PumpJournalError, TurnScope},
    store::{
        AdapterInboundBatchCommand, AdapterInboundEvent, AdapterStoreOwner, ItemIngressCommand,
    },
};

mod hashing;

pub(super) use hashing::digest;
use hashing::{CountingDigest, hex};

pub(super) fn build_command(
    intent: &PumpIntent,
    events: Vec<DurableAdapterEvent>,
    actual_bytes: usize,
) -> Result<AdapterInboundBatchCommand, PumpJournalError> {
    let retained_bytes = preflight(intent, &events, actual_bytes)?;
    let mut converted = Vec::new();
    converted
        .try_reserve_exact(events.len())
        .map_err(|_| PumpJournalError::Rejected)?;
    for event in events {
        converted.push(convert(event)?);
    }
    AdapterInboundBatchCommand::new(
        AdapterStoreOwner {
            project_id: intent.project_id.clone(),
            chat_id: intent.chat_id.clone(),
            session_id: intent.session_id.clone(),
            runtime_epoch: intent.runtime_epoch,
            turn_id: intent.active_turn.turn_id.clone(),
            binding_digest: hex(&intent.binding_digest),
            external_session_id: intent.external_session_id.clone(),
        },
        actual_bytes,
        retained_bytes,
        converted,
    )
    .map_err(|_| PumpJournalError::Rejected)
}

fn preflight(
    intent: &PumpIntent,
    events: &[DurableAdapterEvent],
    actual_bytes: usize,
) -> Result<usize, PumpJournalError> {
    if events.is_empty()
        || events.len() > LIMITS.queues.journal_facts_per_transaction as usize
        || actual_bytes == 0
        || actual_bytes > LIMITS.queues.journal_bytes_per_transaction as usize
    {
        return Err(PumpJournalError::Rejected);
    }
    let mut retained = owner_bytes(intent)
        .checked_add(
            events
                .len()
                .saturating_mul(std::mem::size_of::<AdapterInboundEvent>()),
        )
        .ok_or(PumpJournalError::Rejected)?;
    for event in events {
        retained = retained
            .checked_add(event_bytes(intent, event)?)
            .filter(|bytes| *bytes <= LIMITS.queues.journal_bytes_per_transaction as usize)
            .ok_or(PumpJournalError::Rejected)?;
    }
    Ok(retained)
}

fn event_bytes(
    intent: &PumpIntent,
    event: &DurableAdapterEvent,
) -> Result<usize, PumpJournalError> {
    match event {
        DurableAdapterEvent::Turn { scope, .. } => {
            verify_scope(intent, scope)?;
            Ok(0)
        }
        DurableAdapterEvent::Item {
            scope,
            external_item_id,
            payload,
            payload_digest,
            ..
        }
        | DurableAdapterEvent::RuntimePayload {
            scope,
            external_item_id,
            payload,
            payload_digest,
            ..
        } => {
            verify_scope(intent, scope)?;
            if matches!(
                event,
                DurableAdapterEvent::RuntimePayload { payload, .. }
                    if !matches!(
                        payload,
                        NormalizedPayload::Error { .. }
                            | NormalizedPayload::Unsupported { .. }
                    )
            ) {
                return Err(PumpJournalError::Rejected);
            }
            let payload_bytes = verify_payload(payload, payload_digest)?;
            Ok(scope_heap_bytes(scope)
                + external_item_id.as_str().len()
                + payload_digest.len()
                + payload_bytes)
        }
        DurableAdapterEvent::UsageSnapshot {
            scope,
            input_tokens,
            output_tokens,
            context_window,
            ..
        } => {
            verify_scope(intent, scope)?;
            let payload = NormalizedPayload::Usage {
                input_tokens: *input_tokens,
                output_tokens: *output_tokens,
                context_window: *context_window,
            };
            let payload_bytes = serde_json::to_vec(&payload)
                .map_err(|_| PumpJournalError::Rejected)?
                .len();
            if payload_bytes > LIMITS.wire.normalized_item_bytes as usize {
                return Err(PumpJournalError::Rejected);
            }
            Ok(scope_heap_bytes(scope) + payload_bytes)
        }
        DurableAdapterEvent::ActivityItem(activity) => {
            super::activity::event_bytes(intent, activity)
        }
        DurableAdapterEvent::TerminalReason {
            scope,
            message,
            remediation,
            ..
        } => super::terminal_reason::event_bytes(
            intent,
            scope,
            message.as_ref(),
            remediation.as_ref(),
        ),
        DurableAdapterEvent::FilePair {
            scope,
            external_item_id,
            old_text,
            new_text,
            ..
        } => {
            verify_scope(intent, scope)?;
            if old_text.len() > crate::acp::file_diff::DIFF_SIDE_BYTES_MAX
                || new_text.len() > crate::acp::file_diff::DIFF_SIDE_BYTES_MAX
            {
                return Err(PumpJournalError::Rejected);
            }
            Ok(scope_heap_bytes(scope)
                + external_item_id.as_str().len()
                + old_text.len()
                + new_text.len())
        }
        DurableAdapterEvent::PermissionRequest { intent: deny, .. } => {
            if deny.project_id != intent.project_id
                || deny.chat_id != intent.chat_id
                || deny.session_id != intent.session_id
                || deny.turn_id != intent.active_turn.turn_id
                || deny.runtime_epoch != intent.runtime_epoch
                || deny.binding_digest != hex(&intent.binding_digest)
                || deny.normalized_payload_json.len()
                    > LIMITS.wire.permission_payload_bytes as usize
                || digest(deny.normalized_payload_json.as_bytes()) != deny.normalized_payload_digest
            {
                return Err(PumpJournalError::Rejected);
            }
            Ok(deny.request_id.as_str().len()
                + deny.project_id.as_str().len()
                + deny.chat_id.as_str().len()
                + deny.session_id.as_str().len()
                + deny.turn_id.as_str().len()
                + deny.external_request_id.as_str().len()
                + deny.binding_digest.len()
                + deny.normalized_payload_json.len()
                + deny.payload_digest.len()
                + deny.normalized_payload_digest.len())
        }
        DurableAdapterEvent::PermissionDeny { intent: deny, .. } => {
            if deny.project_id != intent.project_id
                || deny.chat_id != intent.chat_id
                || deny.session_id != intent.session_id
                || deny.turn_id != intent.active_turn.turn_id
                || deny.runtime_epoch != intent.runtime_epoch
                || deny.binding_digest != hex(&intent.binding_digest)
                || deny.normalized_payload_json.len()
                    > LIMITS.wire.permission_payload_bytes as usize
                || digest(deny.normalized_payload_json.as_bytes()) != deny.normalized_payload_digest
            {
                return Err(PumpJournalError::Rejected);
            }
            Ok(deny.request_id.as_str().len()
                + deny.project_id.as_str().len()
                + deny.chat_id.as_str().len()
                + deny.session_id.as_str().len()
                + deny.turn_id.as_str().len()
                + deny.external_request_id.as_str().len()
                + deny.binding_digest.len()
                + deny.normalized_payload_json.len()
                + deny.payload_digest.len()
                + deny.normalized_payload_digest.len())
        }
        DurableAdapterEvent::PermissionResolved {
            scope,
            request_id,
            state,
            ..
        } => {
            verify_scope(intent, scope)?;
            if !state.is_terminal() {
                return Err(PumpJournalError::Rejected);
            }
            Ok(scope_heap_bytes(scope) + request_id.as_str().len())
        }
    }
}

fn convert(event: DurableAdapterEvent) -> Result<AdapterInboundEvent, PumpJournalError> {
    Ok(match event {
        DurableAdapterEvent::Turn {
            state, now_millis, ..
        } => AdapterInboundEvent::Turn { state, now_millis },
        DurableAdapterEvent::Item {
            scope,
            external_item_id,
            lifecycle,
            tool_state,
            payload,
            payload_digest,
            now_millis,
        } => AdapterInboundEvent::Item(ItemIngressCommand {
            project_id: scope.project_id,
            chat_id: scope.chat_id,
            session_id: scope.session_id,
            turn_id: scope.turn_id,
            runtime_epoch: scope.runtime_epoch,
            external_item_id,
            activity_id: None,
            payload,
            payload_digest,
            lifecycle,
            tool_state,
            now_millis,
        }),
        DurableAdapterEvent::ActivityItem(activity) => super::activity::convert(*activity)?,
        DurableAdapterEvent::UsageSnapshot {
            scope: _,
            input_tokens,
            output_tokens,
            context_window,
            now_millis,
        } => AdapterInboundEvent::UsageSnapshot(crate::acp::store::TurnUsageSnapshotCommand {
            input_tokens,
            output_tokens,
            context_window,
            now_millis,
        }),
        DurableAdapterEvent::TerminalReason {
            reason,
            message,
            remediation,
            now_millis,
            ..
        } => super::terminal_reason::convert(reason, message, remediation, now_millis),
        DurableAdapterEvent::FilePair {
            scope,
            external_item_id,
            old_text,
            new_text,
            now_millis,
        } => AdapterInboundEvent::FilePair(crate::acp::store::FilePairCommand {
            project_id: scope.project_id,
            chat_id: scope.chat_id,
            session_id: scope.session_id,
            turn_id: scope.turn_id,
            runtime_epoch: scope.runtime_epoch,
            external_item_id,
            old_text,
            new_text,
            now_millis,
        }),
        DurableAdapterEvent::PermissionRequest { intent, now_millis } => {
            AdapterInboundEvent::PermissionRequest { intent, now_millis }
        }
        DurableAdapterEvent::PermissionDeny { intent, now_millis } => {
            AdapterInboundEvent::PermissionDeny { intent, now_millis }
        }
        DurableAdapterEvent::PermissionResolved {
            scope,
            request_id,
            state,
            now_millis,
        } => AdapterInboundEvent::PermissionResolved(crate::acp::store::RequestStateCommand {
            project_id: scope.project_id,
            chat_id: scope.chat_id,
            session_id: scope.session_id,
            turn_id: scope.turn_id,
            runtime_epoch: scope.runtime_epoch,
            request_id,
            state,
            now_millis,
        }),
        DurableAdapterEvent::RuntimePayload {
            scope,
            external_item_id,
            payload,
            payload_digest,
            now_millis,
        } => AdapterInboundEvent::Item(ItemIngressCommand {
            project_id: scope.project_id,
            chat_id: scope.chat_id,
            session_id: scope.session_id,
            turn_id: scope.turn_id,
            runtime_epoch: scope.runtime_epoch,
            external_item_id,
            activity_id: None,
            lifecycle: runtime_lifecycle(&payload).ok_or(PumpJournalError::Rejected)?,
            tool_state: None,
            payload,
            payload_digest,
            now_millis,
        }),
    })
}

pub(super) fn verify_scope(intent: &PumpIntent, scope: &TurnScope) -> Result<(), PumpJournalError> {
    if scope.project_id == intent.project_id
        && scope.chat_id == intent.chat_id
        && scope.session_id == intent.session_id
        && scope.runtime_epoch == intent.runtime_epoch
        && scope.turn_id == intent.active_turn.turn_id
        && scope.binding_digest == hex(&intent.binding_digest)
    {
        Ok(())
    } else {
        Err(PumpJournalError::Rejected)
    }
}

pub(super) fn verify_payload(
    payload: &NormalizedPayload,
    expected_digest: &str,
) -> Result<usize, PumpJournalError> {
    let mut sink = CountingDigest::default();
    serde_json::to_writer(&mut sink, payload).map_err(|_| PumpJournalError::Rejected)?;
    sink.matched(expected_digest)
        .ok_or(PumpJournalError::Rejected)
}

fn owner_bytes(intent: &PumpIntent) -> usize {
    intent.project_id.as_str().len()
        + intent.chat_id.as_str().len()
        + intent.session_id.as_str().len()
        + intent.active_turn.turn_id.as_str().len()
        + intent.external_session_id.as_str().len()
        + intent.binding_digest.len() * 2
}

pub(super) fn scope_heap_bytes(scope: &TurnScope) -> usize {
    scope.project_id.as_str().len()
        + scope.chat_id.as_str().len()
        + scope.session_id.as_str().len()
        + scope.turn_id.as_str().len()
        + scope.binding_digest.len()
}

fn runtime_lifecycle(payload: &NormalizedPayload) -> Option<crate::acp::lifecycle::ItemLifecycle> {
    use crate::acp::lifecycle::ItemLifecycle;
    match payload {
        NormalizedPayload::Error { .. } => Some(ItemLifecycle::Failed),
        NormalizedPayload::Unsupported { .. } => Some(ItemLifecycle::Unsupported),
        _ => None,
    }
}
