use crate::acp::{
    contract::ClientIntentId,
    lifecycle::TurnTerminalOutcome,
    runtime::TurnNotice,
    store::{AdapterTurnCursor, BeginCancelCommand, MutationDisposition},
};

use super::*;

impl ActiveDispatch {
    /// Commits the exact cancel intent before a caller may enqueue adapter
    /// control. Replaying the same client intent is idempotent in SQLite.
    pub(crate) async fn begin_cancel(
        &mut self,
        client_intent_id: ClientIntentId,
        now_millis: i64,
    ) -> Result<TurnMutation, CoordinatorError> {
        self.command.now_millis = now_millis;
        let commit = self
            .journal
            .enqueue_one(
                JournalLane::Control,
                StoreMutation::BeginCancel(BeginCancelCommand {
                    turn: self.command.clone(),
                    client_intent_id,
                }),
                deadline(now_millis, 5_000),
            )?
            .committed()
            .await?;
        let mutation = one_turn(commit)?;
        self.accept_mutation(&mutation)?;
        Ok(mutation)
    }

    /// Accepts only the store-minted cursor returned by the exact adapter
    /// batch. A matching durable terminal releases the scheduler lease once;
    /// duplicate terminal receipts are harmless and stale/foreign cursors do
    /// not advertise capacity.
    pub(crate) fn accept_adapter_commit(
        &mut self,
        cursor: &AdapterTurnCursor,
    ) -> Result<MutationDisposition, CoordinatorError> {
        if cursor.turn_id() != &self.command.turn_id {
            return Err(CoordinatorError::AdapterCursorForeign);
        }
        if cursor.revision() < self.command.expected_revision
            || cursor.projection_revision() < self.accepted_projection_revision
        {
            return Err(CoordinatorError::AdapterCursorStale);
        }
        let disposition = if cursor.revision() == self.command.expected_revision {
            if cursor.state() != self.accepted_state
                || cursor.delivery_state() != self.accepted_delivery
            {
                return Err(CoordinatorError::AdapterCursorStale);
            }
            MutationDisposition::Duplicate
        } else {
            if self.lease.is_none() {
                return Err(CoordinatorError::AdapterCursorStale);
            }
            self.command.expected_revision = cursor.revision();
            self.accepted_state = cursor.state();
            self.accepted_delivery = cursor.delivery_state();
            MutationDisposition::Applied
        };
        self.accepted_projection_revision = cursor.projection_revision();
        if cursor.state().is_terminal()
            && let Some(lease) = self.lease.take()
        {
            lease.terminal_committed()?;
        }
        Ok(disposition)
    }
}

impl ActiveDispatch {
    pub(crate) fn turn_id(&self) -> &crate::acp::contract::SageTurnId {
        &self.command.turn_id
    }

    pub(crate) fn is_terminal(&self) -> bool {
        self.accepted_state.is_terminal()
    }

    pub(crate) fn state(&self) -> crate::acp::lifecycle::TurnState {
        self.accepted_state
    }

    pub(crate) fn projection_retry_pending(&self) -> bool {
        self.projection_retry_pending
    }

    /// Resolves one retained commit-unknown Turn mutation by a bounded native
    /// lookup. It never replays the mutation or prompt.
    pub(crate) async fn reconcile_commit_unknown(
        &mut self,
        now_millis: i64,
    ) -> Result<TurnMutation, CoordinatorError> {
        if self.commit_unknown_reconciliations >= 2 {
            return Err(CoordinatorError::RetryExhausted);
        }
        self.commit_unknown_reconciliations += 1;
        self.command.now_millis = now_millis;
        let commit = self
            .journal
            .enqueue_one(
                JournalLane::Control,
                StoreMutation::LookupActiveTurn(self.command.clone()),
                deadline(now_millis, 5_000),
            )?
            .committed()
            .await?;
        let mutation = match commit.results.as_slice() {
            [StoreMutationResult::ActiveTurnLookup(Some(mutation))]
                if mutation.projection_revision == commit.projection_revision
                    && mutation.revision >= self.command.expected_revision =>
            {
                mutation.clone()
            }
            _ => return Err(CoordinatorError::JournalProtocol),
        };
        self.command.expected_revision = mutation.revision;
        self.accepted_state = mutation.state;
        self.accepted_delivery = mutation.delivery_state;
        self.accepted_projection_revision = mutation.projection_revision;
        if mutation.state.is_terminal()
            && let Some(lease) = self.lease.take()
        {
            lease.terminal_committed()?;
        }
        self.journal.release_unknown_reservations();
        Ok(mutation)
    }

    pub(crate) fn retry_projection(&mut self) -> Result<(), CoordinatorError> {
        if !self.projection_retry_pending {
            return Err(CoordinatorError::Intent);
        }
        self.publish_current_projection()
    }

    pub(crate) async fn begin_send(
        &mut self,
        now_millis: i64,
    ) -> Result<TurnMutation, CoordinatorError> {
        if self.attempts >= 2 {
            return Err(CoordinatorError::RetryExhausted);
        }
        self.command.now_millis = now_millis;
        let commit = self
            .journal
            .enqueue_one(
                JournalLane::Data,
                StoreMutation::BeginSend(self.command.clone()),
                deadline(now_millis, 30_000),
            )?
            .committed()
            .await?;
        let mutation = one_turn(commit)?;
        self.attempts += 1;
        self.accept_mutation(&mutation)?;
        Ok(mutation)
    }

    pub(crate) async fn observe(
        &mut self,
        observation: SendObservation,
        terminal_reason: Option<TurnTerminalOutcome>,
        now_millis: i64,
    ) -> Result<TurnMutation, CoordinatorError> {
        self.command.now_millis = now_millis;
        let lane = if matches!(observation, SendObservation::ProcessLost) {
            JournalLane::Control
        } else {
            JournalLane::Data
        };
        let commit = self
            .journal
            .enqueue_one(
                lane,
                StoreMutation::ObserveSend {
                    command: self.command.clone(),
                    observation,
                    terminal_reason,
                },
                deadline(
                    now_millis,
                    if lane == JournalLane::Control {
                        5_000
                    } else {
                        30_000
                    },
                ),
            )?
            .committed()
            .await?;
        let mutation = one_turn(commit)?;
        let acceptance = self.accept_mutation(&mutation);
        if mutation.state.is_terminal()
            && let Some(lease) = self.lease.take()
        {
            lease.terminal_committed()?;
        }
        acceptance?;
        Ok(mutation)
    }

    fn accept_mutation(&mut self, mutation: &TurnMutation) -> Result<(), CoordinatorError> {
        self.command.expected_revision = mutation.revision;
        self.accepted_state = mutation.state;
        self.accepted_delivery = mutation.delivery_state;
        self.accepted_projection_revision = mutation.projection_revision;
        self.projection_retry_pending = true;
        self.publish_current_projection()
    }

    fn publish_current_projection(&mut self) -> Result<(), CoordinatorError> {
        let command = &self.command;
        let disposition = self.publisher.publish_committed_with(
            ProjectionChangedV1 {
                project_id: command.project_id.clone(),
                chat_id: command.chat_id.clone(),
                session_id: command.session_id.clone(),
                projection_revision: self.accepted_projection_revision,
                child_projection_epoch: 0,
            },
            TurnNotice::possible_if(self.accepted_state.is_notified_outcome()),
        )?;
        if matches!(
            disposition,
            ProjectionPublishStatus::BootstrapRequired | ProjectionPublishStatus::RecoveryRequired
        ) {
            return Err(CoordinatorError::BootstrapRequired);
        }
        self.projection_retry_pending = false;
        Ok(())
    }
}

fn one_turn(commit: StoreCommit) -> Result<TurnMutation, CoordinatorError> {
    match commit.results.as_slice() {
        [StoreMutationResult::Turn(mutation)]
            if commit.projection_revision == mutation.projection_revision =>
        {
            Ok(mutation.clone())
        }
        _ => Err(CoordinatorError::JournalProtocol),
    }
}
