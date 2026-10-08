use std::{future::Future, pin::Pin, sync::Arc};

use crate::acp::{
    lifecycle::ServerRequestState,
    store::{RequestStateCommand, StoreError, StoreMutation, StoreMutationResult},
};

use super::{
    adapter_pump::{
        AdapterBatchJournal, DurableAdapterEvent, JournalReceipt, PumpIntent, PumpJournalError,
    },
    journal::{JournalError, JournalHandle, JournalLane},
    process_driver::{DriverClock, SystemDriverClock},
};

mod activity;
mod command;
mod terminal_reason;

#[cfg(test)]
use crate::acp::limits::LIMITS;
#[cfg(test)]
use crate::acp::runtime::adapter_pump::TurnScope;
use command::build_command;
#[cfg(test)]
use command::{digest, scope_heap_bytes};

pub(crate) struct SqliteAdapterBatchJournal {
    handle: JournalHandle,
    intent: PumpIntent,
    clock: Arc<dyn DriverClock>,
    #[cfg(feature = "acp-fixture")]
    characterization_observation:
        Option<crate::acp::characterization_observation::CharacterizationObservation>,
    #[cfg(feature = "acp-fixture")]
    first_normalized_item_observed: bool,
}

impl SqliteAdapterBatchJournal {
    pub(crate) fn new(handle: JournalHandle, intent: PumpIntent) -> Self {
        Self {
            handle,
            intent,
            clock: Arc::new(SystemDriverClock),
            #[cfg(feature = "acp-fixture")]
            characterization_observation: None,
            #[cfg(feature = "acp-fixture")]
            first_normalized_item_observed: false,
        }
    }

    #[cfg(feature = "acp-fixture")]
    pub(crate) fn with_characterization_observation(
        mut self,
        observation: crate::acp::characterization_observation::CharacterizationObservation,
    ) -> Self {
        self.characterization_observation = Some(observation);
        self
    }

    #[cfg(test)]
    fn with_clock(handle: JournalHandle, intent: PumpIntent, clock: Arc<dyn DriverClock>) -> Self {
        Self {
            handle,
            intent,
            clock,
            #[cfg(feature = "acp-fixture")]
            characterization_observation: None,
            #[cfg(feature = "acp-fixture")]
            first_normalized_item_observed: false,
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
            #[cfg(feature = "acp-fixture")]
            let has_cancelled_turn = events.iter().any(|event| {
                matches!(
                    event,
                    DurableAdapterEvent::Turn {
                        state: crate::acp::lifecycle::TurnState::Cancelled
                            | crate::acp::lifecycle::TurnState::Interrupted,
                        ..
                    }
                )
            });
            let rejection_context = JournalRejectionContext::from_events(&events);
            let command = build_command(&self.intent, events, actual_bytes)?;
            let ticket = self
                .handle
                .enqueue_one(
                    JournalLane::Data,
                    StoreMutation::AdapterInboundBatch(command),
                    self.next_deadline(),
                )
                .map_err(|error| {
                    log_adapter_batch_rejection("enqueue", &rejection_context, &error);
                    map_journal(error)
                })?;
            let committed = ticket.committed().await.map_err(|error| {
                log_adapter_batch_rejection("commit", &rejection_context, &error);
                map_journal(error)
            })?;
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
            #[cfg(feature = "acp-fixture")]
            let applied = result.disposition == crate::acp::store::MutationDisposition::Applied;
            let cursor = result.into_turn_cursor();
            #[cfg(feature = "acp-fixture")]
            if has_cancelled_turn
                && applied
                && matches!(
                    cursor.state(),
                    crate::acp::lifecycle::TurnState::Cancelled
                        | crate::acp::lifecycle::TurnState::Interrupted
                )
                && let Some(observation) = &self.characterization_observation
            {
                observation.record(
                    crate::acp::characterization_observation::CharacterizationObservationEvent::CancelTerminalCommitted,
                );
            }
            Ok(JournalReceipt::committed(projection_revision, cursor))
        })
    }

    #[cfg(feature = "acp-fixture")]
    fn first_normalized_item_accepted(&mut self) {
        if self.first_normalized_item_observed {
            return;
        }
        self.first_normalized_item_observed = true;
        if let Some(observation) = &self.characterization_observation {
            observation.record(
                crate::acp::characterization_observation::CharacterizationObservationEvent::FirstNormalizedItemAccepted,
            );
        }
    }

    fn transition_permission<'a>(
        &'a mut self,
        intent: &'a crate::acp::runtime::permissions::PermissionIntent,
        state: ServerRequestState,
        now_millis: i64,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<crate::acp::contract::ProjectionRevision, PumpJournalError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            let ticket = self
                .handle
                .enqueue_one(
                    JournalLane::Control,
                    StoreMutation::RequestState(RequestStateCommand {
                        project_id: intent.project_id.clone(),
                        chat_id: intent.chat_id.clone(),
                        session_id: intent.session_id.clone(),
                        turn_id: intent.turn_id.clone(),
                        runtime_epoch: intent.runtime_epoch,
                        request_id: intent.request_id.clone(),
                        state,
                        now_millis,
                    }),
                    self.next_deadline(),
                )
                .map_err(map_journal)?;
            let committed = ticket.committed().await.map_err(map_journal)?;
            let projection_revision = committed.projection_revision;
            let mut results = committed.results;
            let Some(StoreMutationResult::Entity(entity)) = results.pop() else {
                return Err(PumpJournalError::Rejected);
            };
            if !results.is_empty() || entity.projection_revision != projection_revision {
                return Err(PumpJournalError::Rejected);
            }
            Ok(projection_revision)
        })
    }

    fn try_rebind(mut self, intent: PumpIntent) -> Result<Self, (Self, PumpJournalError)> {
        self.intent = intent;
        Ok(self)
    }
}

struct JournalRejectionContext {
    items: Vec<JournalRejectionItem>,
}

struct JournalRejectionItem {
    kind: &'static str,
    external_item_id: String,
}

impl JournalRejectionContext {
    fn from_events(events: &[DurableAdapterEvent]) -> Self {
        let items = events
            .iter()
            .filter_map(|event| match event {
                DurableAdapterEvent::Item {
                    external_item_id,
                    payload,
                    ..
                }
                | DurableAdapterEvent::ChildThreadItem {
                    external_item_id,
                    payload,
                    ..
                }
                | DurableAdapterEvent::RuntimePayload {
                    external_item_id,
                    payload,
                    ..
                } => Some(JournalRejectionItem {
                    kind: payload.kind().as_str(),
                    external_item_id: external_item_id.as_str().to_owned(),
                }),
                DurableAdapterEvent::ChildLiveItem {
                    item_id, payload, ..
                } => Some(JournalRejectionItem {
                    kind: payload.kind().as_str(),
                    external_item_id: item_id.as_str().to_owned(),
                }),
                DurableAdapterEvent::ActivityItem(activity) => Some(JournalRejectionItem {
                    kind: activity.payload.kind().as_str(),
                    external_item_id: activity.external_item_id.as_str().to_owned(),
                }),
                DurableAdapterEvent::FilePair {
                    external_item_id, ..
                } => Some(JournalRejectionItem {
                    kind: "fileChangeDiff",
                    external_item_id: external_item_id.as_str().to_owned(),
                }),
                DurableAdapterEvent::PermissionRequest { intent, .. }
                | DurableAdapterEvent::PermissionDeny { intent, .. } => {
                    Some(JournalRejectionItem {
                        kind: "permissionRequest",
                        external_item_id: intent.external_request_id.as_str().to_owned(),
                    })
                }
                DurableAdapterEvent::PermissionResolved { request_id, .. } => {
                    Some(JournalRejectionItem {
                        kind: "permissionResolved",
                        external_item_id: request_id.as_str().to_owned(),
                    })
                }
                DurableAdapterEvent::UsageSnapshot { .. } => Some(JournalRejectionItem {
                    kind: "usage",
                    external_item_id: "turn-scoped".to_owned(),
                }),
                DurableAdapterEvent::TerminalReason { .. } => Some(JournalRejectionItem {
                    kind: "terminalReason",
                    external_item_id: "turn-scoped".to_owned(),
                }),
                DurableAdapterEvent::Turn { .. } => None,
            })
            .collect();
        Self { items }
    }
}

fn log_adapter_batch_rejection(
    journal_phase: &'static str,
    context: &JournalRejectionContext,
    error: &JournalError,
) {
    let (sqlite_code, sqlite_extended_code, sqlite_message) = match error {
        JournalError::Store(StoreError::Database(rusqlite::Error::SqliteFailure(
            sqlite,
            message,
        ))) => (
            format!("{:?}", sqlite.code),
            sqlite.extended_code.to_string(),
            message.as_deref().unwrap_or("none"),
        ),
        _ => ("none".to_owned(), "none".to_owned(), "none"),
    };

    if context.items.is_empty() {
        tracing::warn!(
            journal_phase,
            journal_rejection = journal_error_variant(error),
            journal_error = ?error,
            sqlite_code,
            sqlite_extended_code,
            sqlite_message,
            "ACP adapter journal rejected inbound batch without an item"
        );
    }
    for item in &context.items {
        tracing::warn!(
            journal_phase,
            journal_rejection = journal_error_variant(error),
            journal_error = ?error,
            sqlite_code,
            sqlite_extended_code,
            sqlite_message,
            item_kind = item.kind,
            external_item_id = item.external_item_id,
            "ACP adapter journal rejected inbound item"
        );
    }
}

fn journal_error_variant(error: &JournalError) -> &'static str {
    match error {
        JournalError::Capacity { .. } => "capacity",
        JournalError::BatchLimit => "batchLimit",
        JournalError::Closed => "closed",
        JournalError::SequenceExhausted => "sequenceExhausted",
        JournalError::Deadline => "deadline",
        JournalError::Store(error) => store_error_variant(error),
    }
}

fn store_error_variant(error: &StoreError) -> &'static str {
    match error {
        StoreError::Database(_) => "database",
        StoreError::Capacity { .. } => "capacity",
        StoreError::Conflict { .. } => "conflict",
        StoreError::InvalidTransition { .. } => "invalidTransition",
        StoreError::TerminalConflict { .. } => "terminalConflict",
        StoreError::Corrupt(_) => "corrupt",
        StoreError::PayloadTooLarge => "payloadTooLarge",
        StoreError::BatchShape => "batchShape",
        StoreError::CommitUnknown => "commitUnknown",
        StoreError::CounterExhausted => "counterExhausted",
    }
}

fn map_journal(error: JournalError) -> PumpJournalError {
    match error {
        JournalError::Store(StoreError::CommitUnknown) => PumpJournalError::CommitUnknown,
        _ => PumpJournalError::Rejected,
    }
}

#[cfg(test)]
mod tests;
