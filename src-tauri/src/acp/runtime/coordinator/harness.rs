use sage_acp_protocol::{
    contract::{EntityRevision, SageTurnId},
    lifecycle::{DeliveryState, TurnState},
};

use super::{prepare::prepare_failure, *};

impl<A: ProjectIntentAuthority> SendCoordinator<A> {
    /// Opens the Turn a harness started on its own in the tree's session. It
    /// takes the tree's one Turn slot like a prompt does, so a prompt sent
    /// meanwhile waits for it, and it is running from the start.
    pub(crate) async fn open_harness_turn(
        &self,
        tree: &TreeLease,
        now_millis: i64,
    ) -> Result<ActiveDispatch, Box<PrepareFailure>> {
        let owner = tree.owner();
        let command = TurnMutationCommand {
            project_id: owner.project_id.clone(),
            chat_id: owner.chat_id.clone(),
            session_id: owner.session_id.clone(),
            runtime_epoch: owner.runtime_epoch,
            turn_id: SageTurnId::new(),
            expected_revision: EntityRevision::ZERO,
            now_millis,
        };
        let reservation = tree
            .reserve_turn()
            .map_err(|error| prepare_failure(error.into()))?;
        let committed = match self.journal.enqueue_one(
            JournalLane::Data,
            StoreMutation::OpenHarnessTurn(command.clone()),
            deadline(now_millis, 30_000),
        ) {
            Ok(ticket) => ticket.committed().await,
            Err(error) => Err(error),
        };
        let mutation = match committed {
            Ok(commit) => match one_opened(commit) {
                Ok(mutation) => mutation,
                // The Turn may be durable, so its slot stays charged.
                Err(error) => return Err(prepare_failure(error)),
            },
            Err(error @ JournalError::Store(crate::acp::store::StoreError::CommitUnknown)) => {
                return Err(prepare_failure(error.into()));
            }
            Err(error) => {
                reservation
                    .abort_before_durable_commit()
                    .map_err(|scheduler| prepare_failure(scheduler.into()))?;
                return Err(prepare_failure(error.into()));
            }
        };
        let lease = reservation
            .bind_retained(command.turn_id.clone())
            .map_err(|(_, error)| prepare_failure(error.into()))?;
        let dispatch = ActiveDispatch {
            journal: self.journal.clone(),
            publisher: Arc::clone(&self.publisher),
            lease: Some(lease),
            command,
            accepted_state: mutation.state,
            accepted_delivery: mutation.delivery_state,
            accepted_projection_revision: mutation.projection_revision,
            projection_retry_pending: false,
            attempts: 0,
            commit_unknown_reconciliations: 0,
        };
        if let Err(error) = publish(
            &self.publisher,
            &dispatch.command.project_id,
            &dispatch.command.chat_id,
            &dispatch.command.session_id,
            dispatch.accepted_projection_revision,
        ) {
            return Err(Box::new(PrepareFailure {
                error,
                recovery: Some(PrepareRecovery::Committed(dispatch)),
            }));
        }
        Ok(dispatch)
    }
}

fn one_opened(commit: StoreCommit) -> Result<TurnMutation, CoordinatorError> {
    match commit.results.as_slice() {
        [StoreMutationResult::Turn(mutation)]
            if commit.projection_revision == mutation.projection_revision
                && mutation.disposition == MutationDisposition::Applied
                && mutation.state == TurnState::Running
                && mutation.delivery_state == DeliveryState::Correlated =>
        {
            Ok(mutation.clone())
        }
        _ => Err(CoordinatorError::JournalProtocol),
    }
}
