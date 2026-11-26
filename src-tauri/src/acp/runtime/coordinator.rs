use std::sync::Arc;

use crate::acp::{
    ipc::{IntentSnapshot, ProjectionChangedV1, validate_base_intent},
    store::{
        MutationDisposition, PrepareTurnCommand, PreparedTurn, SendObservation, StoreCommit,
        StoreMutation, StoreMutationResult, TurnMutation, TurnMutationCommand,
    },
};
use sage_acp_protocol::{
    contract::ProjectionRevision,
    scheduler::{RunningTurnLease, SchedulerError, TreeLease, TurnReservation},
};

mod harness;
mod prepare;
mod turn_control;

pub(crate) use prepare::{PrepareFailure, PrepareRecovery};
use prepare::{build_active_dispatch, one_prepared, prepare_failure, publish};

use super::{
    journal::{JournalError, JournalHandle, JournalLane},
    projection_publisher::{ProjectionPublishError, ProjectionPublishStatus, ProjectionPublisher},
};

pub(crate) trait ProjectIntentAuthority: Send + Sync {
    fn current_snapshot(&self, command: &PrepareTurnCommand) -> Option<IntentSnapshot>;
    /// The harness session mode `owner`'s chat allows now. A Turn the
    /// harness starts on its own opens only in it; with none known, none
    /// opens.
    fn harness_turn_mode(
        &self,
        _owner: &sage_acp_protocol::scheduler::RuntimeOwner,
    ) -> Option<&'static str> {
        None
    }
    /// The access a prompt in `owner`'s chat runs under now. A Turn the
    /// harness starts on its own under another access opens only read-only.
    fn harness_turn_access(
        &self,
        _owner: &sage_acp_protocol::scheduler::RuntimeOwner,
    ) -> Option<sage_acp_protocol::access_mode::SessionAccessMode> {
        None
    }
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
    #[allow(
        dead_code,
        reason = "callers treat a duplicate as settled and do not inspect the stored turn"
    )]
    Duplicate(PreparedTurn),
}

pub(crate) struct ActiveDispatch {
    journal: JournalHandle,
    publisher: Arc<dyn ProjectionPublisher>,
    lease: Option<RunningTurnLease>,
    command: TurnMutationCommand,
    accepted_state: sage_acp_protocol::lifecycle::TurnState,
    accepted_delivery: sage_acp_protocol::lifecycle::DeliveryState,
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
        let (reservation, _, prepared) = self
            .open_reserved(
                reservation,
                StoreMutation::PrepareTurn(command.clone()),
                command.now_millis,
                one_prepared,
            )
            .await?;
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
                return Err(self.ambiguous(
                    reservation,
                    StoreMutation::PrepareTurn(command),
                    error.into(),
                ));
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

fn deadline(now_millis: i64, delta: u64) -> u64 {
    u64::try_from(now_millis)
        .unwrap_or_default()
        .saturating_add(delta)
}
