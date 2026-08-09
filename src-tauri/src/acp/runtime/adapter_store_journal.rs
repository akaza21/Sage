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
                        state: crate::acp::lifecycle::TurnState::Cancelled,
                        ..
                    }
                )
            });
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
            #[cfg(feature = "acp-fixture")]
            let applied = result.disposition == crate::acp::store::MutationDisposition::Applied;
            let cursor = result.into_turn_cursor();
            #[cfg(feature = "acp-fixture")]
            if has_cancelled_turn
                && applied
                && cursor.state() == crate::acp::lifecycle::TurnState::Cancelled
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

fn map_journal(error: JournalError) -> PumpJournalError {
    match error {
        JournalError::Store(StoreError::CommitUnknown) => PumpJournalError::CommitUnknown,
        _ => PumpJournalError::Rejected,
    }
}

#[cfg(test)]
#[path = "adapter_store_journal/tests.rs"]
mod tests;
