//! Owns the exact session tree lease across activation commit, projection
//! recovery, and the ownership-preserving handoff to the Turn runtime.

use std::{future::Future, pin::Pin};

use crate::acp::{
    runtime::{JournalError, SessionCoordinatorError, SessionTreeLease},
    store::{CompleteActivationCommand, StoreError},
};

use super::{ActivationJournal, ActivationJournalError, ActivationReceipt};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CoordinatorActivationHandoff {
    Ready,
    BootstrapRequired,
    RecoveryRequired,
}

pub(crate) enum CoordinatorShutdownHandoff {
    Activation(SessionTreeLease),
    Runtime(SessionTreeLease),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Pending,
    Ready,
    BootstrapRequired,
    RecoveryRequired,
    Rejected,
}

pub(crate) struct CoordinatorActivationJournal {
    lease: Option<SessionTreeLease>,
    state: State,
}

impl CoordinatorActivationJournal {
    pub(crate) fn new(lease: SessionTreeLease) -> Self {
        Self {
            lease: Some(lease),
            state: State::Pending,
        }
    }

    pub(crate) fn lease(&self) -> &SessionTreeLease {
        self.lease
            .as_ref()
            .expect("activation journal owns its lease")
    }

    pub(crate) fn into_handoff(
        mut self,
    ) -> Result<(SessionTreeLease, CoordinatorActivationHandoff), Box<Self>> {
        let handoff = match self.state {
            State::Ready => CoordinatorActivationHandoff::Ready,
            State::BootstrapRequired => CoordinatorActivationHandoff::BootstrapRequired,
            State::RecoveryRequired => CoordinatorActivationHandoff::RecoveryRequired,
            State::Pending | State::Rejected => return Err(Box::new(self)),
        };
        let lease = self
            .lease
            .take()
            .expect("activation journal owns its lease");
        Ok((lease, handoff))
    }

    pub(crate) fn into_teardown_lease(mut self) -> Result<SessionTreeLease, Box<Self>> {
        if !matches!(self.state, State::Pending | State::Rejected) {
            return Err(Box::new(self));
        }
        Ok(self
            .lease
            .take()
            .expect("activation journal owns its lease"))
    }

    pub(crate) fn into_shutdown_handoff(mut self) -> CoordinatorShutdownHandoff {
        let lease = self
            .lease
            .take()
            .expect("activation journal owns its lease");
        match self.state {
            State::Pending | State::Rejected => CoordinatorShutdownHandoff::Activation(lease),
            State::Ready | State::BootstrapRequired | State::RecoveryRequired => {
                CoordinatorShutdownHandoff::Runtime(lease)
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn set_shutdown_phase_for_test(&mut self, runtime: bool) {
        self.state = if runtime {
            State::Ready
        } else {
            State::Rejected
        };
    }

    #[cfg(test)]
    pub(crate) fn set_recovery_phase_for_test(&mut self) {
        self.state = State::RecoveryRequired;
    }

    pub(crate) fn into_pending_lease(mut self) -> Result<SessionTreeLease, Box<Self>> {
        if self.state != State::Pending {
            return Err(Box::new(self));
        }
        Ok(self
            .lease
            .take()
            .expect("activation journal owns its lease"))
    }

    pub(crate) fn into_recovery_lease(mut self) -> Result<SessionTreeLease, Box<Self>> {
        if !matches!(
            self.state,
            State::BootstrapRequired | State::RecoveryRequired
        ) {
            return Err(Box::new(self));
        }
        Ok(self
            .lease
            .take()
            .expect("activation journal owns its lease"))
    }
}

impl ActivationJournal for CoordinatorActivationJournal {
    fn commit_ready<'a>(
        &'a mut self,
        command: CompleteActivationCommand,
    ) -> Pin<Box<dyn Future<Output = Result<ActivationReceipt, ActivationJournalError>> + Send + 'a>>
    {
        Box::pin(async move {
            if self.state != State::Pending {
                return Err(ActivationJournalError::Rejected);
            }
            let result = self
                .lease
                .as_mut()
                .expect("activation journal owns its lease")
                .complete_activation(command)
                .await;
            match result {
                Ok(mutation) => {
                    self.state = State::Ready;
                    Ok(ActivationReceipt {
                        projection_revision: mutation.projection_revision,
                    })
                }
                Err(error) => {
                    let mapped = map_error(error);
                    self.state = match mapped {
                        ActivationJournalError::BootstrapRequired => State::BootstrapRequired,
                        ActivationJournalError::CommitUnknown
                        | ActivationJournalError::RecoveryRequired => State::RecoveryRequired,
                        ActivationJournalError::Rejected => State::Rejected,
                    };
                    Err(mapped)
                }
            }
        })
    }
}

fn map_error(error: SessionCoordinatorError) -> ActivationJournalError {
    match error {
        SessionCoordinatorError::BootstrapRequired => ActivationJournalError::BootstrapRequired,
        SessionCoordinatorError::Projection(_)
        | SessionCoordinatorError::JournalProtocol
        | SessionCoordinatorError::Scheduler(_)
        | SessionCoordinatorError::Journal(JournalError::Closed) => {
            ActivationJournalError::RecoveryRequired
        }
        SessionCoordinatorError::Journal(JournalError::Store(StoreError::CommitUnknown)) => {
            ActivationJournalError::CommitUnknown
        }
        SessionCoordinatorError::Journal(
            JournalError::Capacity { .. }
            | JournalError::BatchLimit
            | JournalError::SequenceExhausted
            | JournalError::Deadline
            | JournalError::Store(_),
        )
        | SessionCoordinatorError::Intent
        | SessionCoordinatorError::Converged => ActivationJournalError::Rejected,
    }
}
