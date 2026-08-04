use std::sync::Arc;

use crate::acp::{
    contract::{EntityRevision, ProjectionRevision},
    intent::{IntentSnapshot, ProjectionChangedV1, validate_base_intent},
    scheduler::{RunningTurnLease, SchedulerError, TreeLease, TurnReservation},
    store::{
        MutationDisposition, PrepareTurnCommand, PreparedTurn, SendObservation, StoreCommit,
        StoreMutation, StoreMutationResult, TurnMutation, TurnMutationCommand,
    },
};

mod prepare;
mod turn_control;

pub(crate) use prepare::{AmbiguousTurnReservation, PrepareFailure, PrepareRecovery};
use prepare::{build_active_dispatch, one_prepared, prepare_failure, publish};

use super::{
    journal::{JournalError, JournalHandle, JournalLane},
    projection_publisher::{ProjectionPublishError, ProjectionPublishStatus, ProjectionPublisher},
};

pub(crate) trait ProjectIntentAuthority: Send + Sync {
    fn current_snapshot(&self, command: &PrepareTurnCommand) -> Option<IntentSnapshot>;
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CoordinatorError {
    #[error("the project/session intent is stale or foreign")]
    Intent,
    #[error(transparent)]
    Scheduler(#[from] SchedulerError),
    #[error(transparent)]
    Journal(#[from] JournalError),
    #[error(transparent)]
    Projection(#[from] ProjectionPublishError),
    #[error("journal returned an unexpected mutation result")]
    JournalProtocol,
    #[error("zero-byte prompt retry is exhausted")]
    RetryExhausted,
    #[error("projection mailbox changed owner; authoritative bootstrap is required")]
    BootstrapRequired,
    #[error("durable adapter Turn cursor belongs to a foreign Turn")]
    AdapterCursorForeign,
    #[error("durable adapter Turn cursor is stale or conflicts with the accepted cursor")]
    AdapterCursorStale,
}

pub(crate) struct SendCoordinator<A> {
    authority: Arc<A>,
    journal: JournalHandle,
    publisher: Arc<dyn ProjectionPublisher>,
}

pub(crate) enum PrepareDispatch {
    Applied(ActiveDispatch),
    Duplicate(PreparedTurn),
}

pub(crate) struct ActiveDispatch {
    journal: JournalHandle,
    publisher: Arc<dyn ProjectionPublisher>,
    lease: Option<RunningTurnLease>,
    command: TurnMutationCommand,
    accepted_state: crate::acp::lifecycle::TurnState,
    accepted_delivery: crate::acp::lifecycle::DeliveryState,
    accepted_projection_revision: ProjectionRevision,
    projection_retry_pending: bool,
    attempts: u8,
    commit_unknown_reconciliations: u8,
}

impl<A: ProjectIntentAuthority> SendCoordinator<A> {
    pub(crate) fn new(
        authority: Arc<A>,
        journal: JournalHandle,
        publisher: Arc<dyn ProjectionPublisher>,
    ) -> Self {
        Self {
            authority,
            journal,
            publisher,
        }
    }

    pub(crate) async fn prepare(
        &self,
        tree: &TreeLease,
        command: PrepareTurnCommand,
    ) -> Result<PrepareDispatch, Box<PrepareFailure>> {
        let snapshot = self
            .authority
            .current_snapshot(&command)
            .ok_or_else(|| prepare_failure(CoordinatorError::Intent))?;
        validate_base_intent(
            &command.project_id,
            command.project_generation,
            &command.chat_id,
            command.expected_projection_revision,
            &snapshot,
        )
        .map_err(|_| prepare_failure(CoordinatorError::Intent))?;
        if snapshot.session_id.as_ref() != Some(&command.session_id)
            || snapshot.runtime_epoch != Some(command.runtime_epoch)
        {
            return Err(prepare_failure(CoordinatorError::Intent));
        }

        if let Some(prepared) = self.lookup(&command).await.map_err(prepare_failure)? {
            return Ok(PrepareDispatch::Duplicate(prepared));
        }
        if snapshot.turn_id.is_some() {
            return Err(prepare_failure(SchedulerError::TurnCapacity.into()));
        }

        // This reservation happens before the journal can create a Turn.
        let tree_owner = tree.owner();
        if tree_owner.project_id != command.project_id
            || tree_owner.chat_id != command.chat_id
            || tree_owner.session_id != command.session_id
            || tree_owner.runtime_epoch != command.runtime_epoch
        {
            return Err(prepare_failure(CoordinatorError::Intent));
        }
        let reservation = match tree.reserve_turn() {
            Ok(reservation) => reservation,
            Err(SchedulerError::TurnCapacity) => {
                if let Some(prepared) = self.lookup(&command).await.map_err(prepare_failure)? {
                    return Ok(PrepareDispatch::Duplicate(prepared));
                }
                return Err(prepare_failure(SchedulerError::TurnCapacity.into()));
            }
            Err(error) => return Err(prepare_failure(error.into())),
        };
        let ticket = match self.journal.enqueue_one(
            JournalLane::Data,
            StoreMutation::PrepareTurn(command.clone()),
            deadline(command.now_millis, 30_000),
        ) {
            Ok(ticket) => ticket,
            Err(error) => {
                reservation
                    .abort_before_durable_commit()
                    .map_err(|scheduler| prepare_failure(scheduler.into()))?;
                return Err(prepare_failure(error.into()));
            }
        };
        let commit = match ticket.committed().await {
            Ok(commit) => commit,
            Err(JournalError::Store(crate::acp::store::StoreError::CommitUnknown)) => {
                return Err(Box::new(PrepareFailure {
                    error: JournalError::Store(crate::acp::store::StoreError::CommitUnknown).into(),
                    recovery: Some(PrepareRecovery::Ambiguous(AmbiguousTurnReservation {
                        reservation,
                        journal: self.journal.clone(),
                        publisher: Arc::clone(&self.publisher),
                        command,
                    })),
                }));
            }
            Err(JournalError::Store(error)) => {
                reservation
                    .abort_before_durable_commit()
                    .map_err(|scheduler| prepare_failure(scheduler.into()))?;
                return Err(prepare_failure(JournalError::Store(error).into()));
            }
            Err(JournalError::Deadline) => {
                reservation
                    .abort_before_durable_commit()
                    .map_err(|scheduler| prepare_failure(scheduler.into()))?;
                return Err(prepare_failure(JournalError::Deadline.into()));
            }
            Err(error) => {
                reservation
                    .abort_before_durable_commit()
                    .map_err(|scheduler| prepare_failure(scheduler.into()))?;
                return Err(prepare_failure(error.into()));
            }
        };
        let prepared = match one_prepared(commit) {
            Ok(prepared) => prepared,
            Err(error) => {
                return Err(Box::new(PrepareFailure {
                    error,
                    recovery: Some(PrepareRecovery::Ambiguous(AmbiguousTurnReservation {
                        reservation,
                        journal: self.journal.clone(),
                        publisher: Arc::clone(&self.publisher),
                        command,
                    })),
                }));
            }
        };
        if prepared.disposition == MutationDisposition::Duplicate {
            reservation
                .abort_before_durable_commit()
                .map_err(|error| prepare_failure(error.into()))?;
            publish(
                &self.publisher,
                &command.project_id,
                &command.chat_id,
                &command.session_id,
                prepared.projection_revision,
            )
            .map_err(prepare_failure)?;
            return Ok(PrepareDispatch::Duplicate(prepared));
        }
        let active = match build_active_dispatch(
            reservation,
            &self.journal,
            &self.publisher,
            command,
            &prepared,
        ) {
            Ok(active) => active,
            Err((reservation, command, error)) => {
                return Err(Box::new(PrepareFailure {
                    error: error.into(),
                    recovery: Some(PrepareRecovery::Ambiguous(AmbiguousTurnReservation {
                        reservation,
                        journal: self.journal.clone(),
                        publisher: Arc::clone(&self.publisher),
                        command,
                    })),
                }));
            }
        };
        if let Err(error) = publish(
            &active.publisher,
            &active.command.project_id,
            &active.command.chat_id,
            &active.command.session_id,
            active.accepted_projection_revision,
        ) {
            return Err(Box::new(PrepareFailure {
                error,
                recovery: Some(PrepareRecovery::Committed(active)),
            }));
        }
        Ok(PrepareDispatch::Applied(active))
    }

    async fn lookup(
        &self,
        command: &PrepareTurnCommand,
    ) -> Result<Option<PreparedTurn>, CoordinatorError> {
        let commit = self
            .journal
            .enqueue_one(
                JournalLane::Data,
                StoreMutation::LookupTurn(command.clone()),
                deadline(command.now_millis, 30_000),
            )?
            .committed()
            .await?;
        match commit.results.as_slice() {
            [StoreMutationResult::Lookup(prepared)]
                if prepared.as_ref().is_none_or(|value| {
                    value.projection_revision == commit.projection_revision
                }) =>
            {
                Ok(prepared.clone())
            }
            _ => Err(CoordinatorError::JournalProtocol),
        }
    }
}

pub(crate) fn revision_zero() -> EntityRevision {
    EntityRevision::ZERO
}

fn deadline(now_millis: i64, delta: u64) -> u64 {
    u64::try_from(now_millis)
        .unwrap_or_default()
        .saturating_add(delta)
}
