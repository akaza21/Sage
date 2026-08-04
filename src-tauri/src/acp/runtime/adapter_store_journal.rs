use std::{future::Future, io::Write, pin::Pin, sync::Arc};

use sha2::{Digest, Sha256};

use crate::acp::{
    intent::NormalizedPayload,
    limits::LIMITS,
    store::{
        AdapterInboundBatchCommand, AdapterInboundEvent, AdapterStoreOwner, ItemIngressCommand,
        StoreError, StoreMutation, StoreMutationResult,
    },
};

use super::{
    adapter_pump::{
        AdapterBatchJournal, DurableAdapterEvent, JournalReceipt, PumpIntent, PumpJournalError,
        TurnScope,
    },
    journal::{JournalError, JournalHandle, JournalLane},
    process_driver::{DriverClock, SystemDriverClock},
};

pub(crate) struct SqliteAdapterBatchJournal {
    handle: JournalHandle,
    intent: PumpIntent,
    clock: Arc<dyn DriverClock>,
}

impl SqliteAdapterBatchJournal {
    pub(crate) fn new(handle: JournalHandle, intent: PumpIntent) -> Self {
        Self {
            handle,
            intent,
            clock: Arc::new(SystemDriverClock),
        }
    }

    #[cfg(test)]
    fn with_clock(handle: JournalHandle, intent: PumpIntent, clock: Arc<dyn DriverClock>) -> Self {
        Self {
            handle,
            intent,
            clock,
        }
    }

    fn next_deadline(&self) -> u64 {
        u64::try_from(self.clock.now_millis())
            .unwrap_or_default()
            .saturating_add(30_000)
    }
}

impl AdapterBatchJournal for SqliteAdapterBatchJournal {
    fn commit<'a>(
        &'a mut self,
        events: Vec<DurableAdapterEvent>,
        actual_bytes: usize,
    ) -> Pin<Box<dyn Future<Output = Result<JournalReceipt, PumpJournalError>> + Send + 'a>> {
        Box::pin(async move {
            let command = build_command(&self.intent, events, actual_bytes)?;
            let ticket = self
                .handle
                .enqueue_one(
                    JournalLane::Data,
                    StoreMutation::AdapterInboundBatch(command),
                    self.next_deadline(),
                )
                .map_err(map_journal)?;
            let committed = ticket.committed().await.map_err(map_journal)?;
            let projection_revision = committed.projection_revision;
            let mut results = committed.results;
            let Some(StoreMutationResult::AdapterBatch(result)) = results.pop() else {
                return Err(PumpJournalError::Rejected);
            };
            if !results.is_empty()
                || result.projection_revision != projection_revision
                || result.turn_cursor().projection_revision() != projection_revision
            {
                return Err(PumpJournalError::Rejected);
            }
            Ok(JournalReceipt::committed(
                projection_revision,
                result.into_turn_cursor(),
            ))
        })
    }

    fn try_rebind(mut self, intent: PumpIntent) -> Result<Self, (Self, PumpJournalError)> {
        self.intent = intent;
        Ok(self)
    }
}

fn build_command(
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
                        NormalizedPayload::Usage { .. }
                            | NormalizedPayload::Error { .. }
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
        DurableAdapterEvent::PermissionDeny { intent, now_millis } => {
            AdapterInboundEvent::PermissionDeny { intent, now_millis }
        }
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

fn verify_scope(intent: &PumpIntent, scope: &TurnScope) -> Result<(), PumpJournalError> {
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

fn verify_payload(
    payload: &NormalizedPayload,
    expected_digest: &str,
) -> Result<usize, PumpJournalError> {
    let mut sink = CountingDigest::default();
    serde_json::to_writer(&mut sink, payload).map_err(|_| PumpJournalError::Rejected)?;
    if sink.bytes > LIMITS.wire.normalized_item_bytes as usize
        || hex(&sink.digest.finalize()) != expected_digest
    {
        return Err(PumpJournalError::Rejected);
    }
    Ok(sink.bytes)
}

fn owner_bytes(intent: &PumpIntent) -> usize {
    intent.project_id.as_str().len()
        + intent.chat_id.as_str().len()
        + intent.session_id.as_str().len()
        + intent.active_turn.turn_id.as_str().len()
        + intent.external_session_id.as_str().len()
        + intent.binding_digest.len() * 2
}

fn scope_heap_bytes(scope: &TurnScope) -> usize {
    scope.project_id.as_str().len()
        + scope.chat_id.as_str().len()
        + scope.session_id.as_str().len()
        + scope.turn_id.as_str().len()
        + scope.binding_digest.len()
}

fn runtime_lifecycle(payload: &NormalizedPayload) -> Option<crate::acp::lifecycle::ItemLifecycle> {
    use crate::acp::lifecycle::ItemLifecycle;
    match payload {
        NormalizedPayload::Usage { .. } => Some(ItemLifecycle::Completed),
        NormalizedPayload::Error { .. } => Some(ItemLifecycle::Failed),
        NormalizedPayload::Unsupported { .. } => Some(ItemLifecycle::Unsupported),
        _ => None,
    }
}

fn map_journal(error: JournalError) -> PumpJournalError {
    match error {
        JournalError::Store(StoreError::CommitUnknown) => PumpJournalError::CommitUnknown,
        _ => PumpJournalError::Rejected,
    }
}

#[derive(Default)]
struct CountingDigest {
    bytes: usize,
    digest: Sha256,
}

impl Write for CountingDigest {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.bytes = self
            .bytes
            .checked_add(bytes.len())
            .filter(|total| *total <= LIMITS.wire.normalized_item_bytes as usize)
            .ok_or_else(|| std::io::Error::other("normalized payload capacity"))?;
        self.digest.update(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn digest(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
#[path = "adapter_store_journal/tests.rs"]
mod tests;
