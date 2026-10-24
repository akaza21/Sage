use super::*;

pub(crate) struct PrepareFailure {
    pub(crate) error: CoordinatorError,
    pub(crate) recovery: Option<PrepareRecovery>,
}

pub(crate) enum PrepareRecovery {
    Ambiguous(AmbiguousTurnReservation),
    Committed(ActiveDispatch),
}

pub(crate) struct AmbiguousTurnReservation {
    pub(super) reservation: TurnReservation,
    pub(super) journal: JournalHandle,
    pub(super) publisher: Arc<dyn ProjectionPublisher>,
    pub(super) command: PrepareTurnCommand,
}

impl std::fmt::Debug for PrepareFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PrepareFailure")
            .field("error", &self.error)
            .field("recovery_retained", &self.recovery.is_some())
            .finish()
    }
}

impl PrepareRecovery {
    // The error owns the exact scheduler reservation/dispatch so callers cannot
    // accidentally release capacity while authoritative recovery is pending.
    #[allow(clippy::result_large_err)]
    pub(crate) fn committed_dispatch(self) -> Result<ActiveDispatch, Self> {
        match self {
            Self::Committed(dispatch) => Ok(dispatch),
            recovery => Err(recovery),
        }
    }

    pub(crate) fn ambiguous(&self) -> Option<&AmbiguousTurnReservation> {
        match self {
            Self::Ambiguous(reservation) => Some(reservation),
            Self::Committed(_) => None,
        }
    }
}

impl AmbiguousTurnReservation {
    pub(crate) fn command(&self) -> &PrepareTurnCommand {
        &self.command
    }

    // Preserve the complete reservation on the error path; boxing here would
    // add an allocation to a state that is already bounded by the scheduler.
    #[allow(clippy::result_large_err)]
    pub(crate) fn resolve_committed(
        self,
        prepared: PreparedTurn,
    ) -> Result<ActiveDispatch, (Self, CoordinatorError)> {
        if prepared.disposition != MutationDisposition::Applied {
            return Err((self, CoordinatorError::JournalProtocol));
        }
        let Self {
            reservation,
            journal,
            publisher,
            command,
        } = self;
        match build_active_dispatch(reservation, &journal, &publisher, command, &prepared) {
            Ok(dispatch) => Ok(dispatch),
            Err((reservation, command, error)) => Err((
                Self {
                    reservation,
                    journal,
                    publisher,
                    command,
                },
                error.into(),
            )),
        }
    }

    #[cfg(test)]
    pub(crate) fn abort_proven_absent(self) -> Result<(), CoordinatorError> {
        self.reservation.abort_before_durable_commit()?;
        Ok(())
    }
}

pub(super) fn prepare_failure(error: CoordinatorError) -> Box<PrepareFailure> {
    Box::new(PrepareFailure {
        error,
        recovery: None,
    })
}

// The error tuple deliberately returns both single-owner values without an
// extra allocation so the caller can recover or release them exactly once.
#[allow(clippy::result_large_err)]
pub(super) fn build_active_dispatch(
    reservation: TurnReservation,
    journal: &JournalHandle,
    publisher: &Arc<dyn ProjectionPublisher>,
    command: PrepareTurnCommand,
    prepared: &PreparedTurn,
) -> Result<ActiveDispatch, (TurnReservation, PrepareTurnCommand, SchedulerError)> {
    let lease = match reservation.bind_retained(prepared.turn_id.clone()) {
        Ok(lease) => lease,
        Err((reservation, error)) => return Err((reservation, command, error)),
    };
    Ok(ActiveDispatch {
        journal: journal.clone(),
        publisher: Arc::clone(publisher),
        lease: Some(lease),
        command: TurnMutationCommand {
            project_id: command.project_id,
            chat_id: command.chat_id,
            session_id: command.session_id,
            runtime_epoch: command.runtime_epoch,
            turn_id: prepared.turn_id.clone(),
            expected_revision: prepared.turn_revision,
            now_millis: command.now_millis,
        },
        accepted_state: crate::acp::lifecycle::TurnState::Queued,
        accepted_delivery: crate::acp::lifecycle::DeliveryState::NotWritten,
        accepted_projection_revision: prepared.projection_revision,
        projection_retry_pending: false,
        attempts: 0,
        commit_unknown_reconciliations: 0,
    })
}

pub(super) fn one_prepared(commit: StoreCommit) -> Result<PreparedTurn, CoordinatorError> {
    match commit.results.as_slice() {
        [StoreMutationResult::Prepared(prepared)]
            if commit.projection_revision == prepared.projection_revision =>
        {
            Ok(prepared.clone())
        }
        _ => Err(CoordinatorError::JournalProtocol),
    }
}

pub(super) fn publish(
    publisher: &Arc<dyn ProjectionPublisher>,
    project_id: &crate::acp::contract::SageProjectId,
    chat_id: &crate::acp::contract::SageChatId,
    session_id: &crate::acp::contract::SageAgentSessionId,
    projection_revision: crate::acp::contract::ProjectionRevision,
) -> Result<(), CoordinatorError> {
    let disposition = publisher.publish_committed(ProjectionChangedV1 {
        project_id: project_id.clone(),
        chat_id: chat_id.clone(),
        session_id: session_id.clone(),
        projection_revision,
        child_projection_epoch: 0,
    })?;
    if matches!(
        disposition,
        ProjectionPublishStatus::BootstrapRequired | ProjectionPublishStatus::RecoveryRequired
    ) {
        Err(CoordinatorError::BootstrapRequired)
    } else {
        Ok(())
    }
}
