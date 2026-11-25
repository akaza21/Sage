use sage_acp_protocol::{
    contract::{EntityRevision, SageTurnId},
    lifecycle::{DeliveryState, TurnState},
};

use super::{prepare::prepare_failure, *};

impl<A: ProjectIntentAuthority> SendCoordinator<A> {
    /// Opens the Turn a harness started on its own in the tree's session,
    /// running in the harness session `mode`, which must be the one the
    /// chat's access allows now. A Turn running under an access opens while
    /// the chat's access is that one, or when it runs read-only. It takes the
    /// tree's one Turn slot like a prompt does, so a prompt sent meanwhile is
    /// refused, and it is running from the start.
    pub(crate) async fn open_harness_turn(
        &self,
        tree: &TreeLease,
        mode: Option<crate::acp::bindings::adapter::HarnessTurnAuthority>,
        now_millis: i64,
    ) -> Result<ActiveDispatch, Box<PrepareFailure>> {
        let owner = tree.owner();
        use crate::acp::bindings::adapter::HarnessTurnAuthority;
        use sage_acp_protocol::access_mode::SessionAccessMode;
        if mode.is_none()
            || matches!(mode, Some(HarnessTurnAuthority::SessionMode(mode)) if self.authority.harness_turn_mode(owner) != Some(mode))
            || matches!(mode, Some(HarnessTurnAuthority::Access(access)) if access != SessionAccessMode::ReadOnly && self.authority.harness_turn_access(owner) != Some(access))
        {
            return Err(prepare_failure(CoordinatorError::Intent));
        }
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
        let (reservation, mutation, opened) = self
            .open_reserved(
                reservation,
                StoreMutation::OpenHarnessTurn(command.clone()),
                now_millis,
                one_opened,
            )
            .await?;
        let lease = match reservation.bind_retained(command.turn_id.clone()) {
            Ok(lease) => lease,
            Err((reservation, error)) => {
                return Err(self.ambiguous(reservation, mutation, error.into()));
            }
        };
        let dispatch = ActiveDispatch {
            journal: self.journal.clone(),
            publisher: Arc::clone(&self.publisher),
            lease: Some(lease),
            command,
            accepted_state: opened.state,
            accepted_delivery: opened.delivery_state,
            accepted_projection_revision: opened.projection_revision,
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
