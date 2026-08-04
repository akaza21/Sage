use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    runtime::{AdapterBatchJournal, SessionTreeLease, ZeroProcessProof},
    scheduler::RuntimeOwner,
    store::{CloseSessionCommand, FinalizeCloseCommand, SendObservation},
};

use super::{
    CoordinatorZeroToken, DriverError, PreparedProcessWrite, PreparedWriteError,
    ProcessTreeControl, StdinTransport, ZeroProcessHandoff,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkerZeroProof {
    owner: RuntimeOwner,
    token: u64,
}

impl WorkerZeroProof {
    pub(super) fn new(owner: RuntimeOwner, token: u64) -> Result<Self, CloseError> {
        if token == 0 {
            return Err(CloseError::Proof);
        }
        Ok(Self { owner, token })
    }

    pub(super) fn actual_bytes(&self) -> Option<usize> {
        self.owner
            .project_id
            .as_str()
            .len()
            .checked_add(self.owner.chat_id.as_str().len())?
            .checked_add(self.owner.session_id.as_str().len())?
            .checked_add(std::mem::size_of::<u64>() * 2)
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CloseError {
    #[error("the close command is stale or foreign")]
    Intent,
    #[error("the session close transition failed")]
    Session(#[from] crate::acp::runtime::SessionCoordinatorError),
    #[error("the active Turn close transition failed")]
    Write(PreparedWriteError),
    #[error("the process tree close transition failed")]
    Driver(DriverError),
    #[error("the zero-process proof token is invalid or exhausted")]
    Proof,
}

pub(crate) struct CloseFailure<S> {
    pub(crate) state: S,
    pub(crate) error: CloseError,
}

pub(crate) struct ClosingProcessWrite<A, J, R, T, C>
where
    A: HarnessAdapter,
    J: AdapterBatchJournal,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
{
    active: PreparedProcessWrite<A, J, R, T, C>,
    command: CloseSessionCommand,
}

pub(crate) struct ActiveCloseBeginState<A, J, R, T, C>
where
    A: HarnessAdapter,
    J: AdapterBatchJournal,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
{
    active: PreparedProcessWrite<A, J, R, T, C>,
    command: CloseSessionCommand,
    close_committed: bool,
    cancel_committed: bool,
    cancel_dispatched: bool,
    cancel_delivery_ambiguous: bool,
    durable_mutation_unknown: bool,
    authoritative_projection: Option<crate::acp::contract::ProjectionRevision>,
}

pub(crate) struct ZeroCloseState<A, J, R, T, C>
where
    A: HarnessAdapter,
    J: AdapterBatchJournal,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
    C::ZeroProof: CoordinatorZeroToken,
{
    active: PreparedProcessWrite<A, J, R, T, C>,
    lease: SessionTreeLease,
    proof: Option<CloseProof<C::ZeroProof>>,
    command: CloseSessionCommand,
    receipt: WorkerZeroProof,
}

enum CloseProof<P> {
    Native(P),
    Validated(ZeroProcessProof),
}

impl<A, J, R, T, C> ActiveCloseBeginState<A, J, R, T, C>
where
    A: HarnessAdapter,
    J: AdapterBatchJournal,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
{
    pub(crate) fn new(
        active: PreparedProcessWrite<A, J, R, T, C>,
        command: CloseSessionCommand,
    ) -> Self {
        let cancel_committed = active.cancel_intent().is_some();
        let cancel_dispatched = active.cancel_was_dispatched();
        let cancel_delivery_ambiguous = active.cancel_requires_convergence();
        let durable_mutation_unknown = active.durable_mutation_unknown();
        Self {
            active,
            command,
            close_committed: false,
            cancel_committed,
            cancel_dispatched,
            cancel_delivery_ambiguous,
            durable_mutation_unknown,
            authoritative_projection: None,
        }
    }

    pub(crate) fn command(&self) -> &CloseSessionCommand {
        &self.command
    }

    pub(crate) async fn begin(
        mut self,
    ) -> Result<ClosingProcessWrite<A, J, R, T, C>, Box<CloseFailure<Self>>> {
        let owner = self.active.turn.driver.runtime_owner().clone();
        if owner.project_id != self.command.project_id
            || owner.chat_id != self.command.chat_id
            || owner.session_id != self.command.session_id
            || owner.runtime_epoch != self.command.runtime_epoch
        {
            return Err(failure(self, CloseError::Intent));
        }
        if self.durable_mutation_unknown {
            let (active, mutation) = match self
                .active
                .reconcile_durable_mutation_unknown(self.command.now_millis)
                .await
            {
                Ok(value) => value,
                Err(write) => {
                    let write = *write;
                    self.active = write.state;
                    return Err(failure(self, CloseError::Write(write.error)));
                }
            };
            self.active = active;
            self.authoritative_projection = Some(mutation.projection_revision);
            self.durable_mutation_unknown = false;
        }
        if !self.close_committed {
            let mut durable_command = self.command.clone();
            if let Some(projection) = self.authoritative_projection {
                durable_command.expected_projection_revision = projection;
            }
            let close_result = match self.active.turn.driver.lease.as_mut() {
                Some(lease) => lease.close_session(durable_command).await,
                None => return Err(failure(self, CloseError::Driver(DriverError::Tree))),
            };
            if let Err(error) = close_result {
                return Err(failure(self, error.into()));
            }
            self.close_committed = true;
        }
        self.active = match self.active.accept_latest_durable_terminal() {
            Ok(active) => active,
            Err(write) => {
                let write = *write;
                self.active = write.state;
                return Err(failure(self, CloseError::Write(write.error)));
            }
        };
        if !self.active.turn.dispatch.is_terminal() && self.active.ready_for_write() {
            self.active = match self
                .active
                .observe(SendObservation::ProcessLost, self.command.now_millis)
                .await
            {
                Ok(active) => active,
                Err(write) => {
                    let write = *write;
                    self.active = write.state;
                    return Err(failure(self, CloseError::Write(write.error)));
                }
            };
        }
        if !self.active.turn.dispatch.is_terminal() {
            let cancel = self.active.cancel_intent().cloned().unwrap_or_else(|| {
                crate::acp::runtime::HostCancelIntent {
                    owner,
                    turn_id: self.active.turn.dispatch.turn_id().clone(),
                    client_intent_id: self.command.client_intent_id.clone(),
                    now_millis: self.command.now_millis,
                }
            });
            if !self.cancel_committed {
                self.active = match self.active.begin_cancel(&cancel).await {
                    Ok(active) => active,
                    Err(write) => {
                        let write = *write;
                        self.active = write.state;
                        return Err(failure(self, CloseError::Write(write.error)));
                    }
                };
                self.cancel_committed = true;
            }
            if !self.cancel_dispatched && !self.cancel_delivery_ambiguous {
                self.active = match self.active.dispatch_cancel(&cancel) {
                    Ok(active) => active,
                    Err(write) => {
                        let mut write = *write;
                        let _ = write.state.turn.driver.request_host_termination();
                        self.active = write.state;
                        self.cancel_delivery_ambiguous = true;
                        return Err(failure(self, CloseError::Write(write.error)));
                    }
                };
                self.cancel_dispatched = true;
            }
        }
        if self.active.turn.driver.teardown_cause().is_none()
            && let Err(error) = self.active.turn.driver.request_host_termination()
        {
            return Err(failure(self, CloseError::Driver(error)));
        }
        Ok(ClosingProcessWrite {
            active: self.active,
            command: self.command,
        })
    }
}

impl<A, J, R, T, C> ClosingProcessWrite<A, J, R, T, C>
where
    A: HarnessAdapter,
    J: AdapterBatchJournal,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
    C::ZeroProof: CoordinatorZeroToken,
{
    pub(crate) async fn converge_zero(
        self,
        token: u64,
        now_millis: i64,
    ) -> Result<ZeroCloseState<A, J, R, T, C>, Box<CloseFailure<Self>>> {
        let Self {
            mut active,
            command,
        } = self;
        active = match active.accept_latest_durable_terminal() {
            Ok(active) => active,
            Err(write) => {
                let write = *write;
                return Err(failure(
                    Self {
                        active: write.state,
                        command,
                    },
                    CloseError::Write(write.error),
                ));
            }
        };
        if !active.turn.dispatch.is_terminal() {
            active = match active
                .observe(SendObservation::ProcessLost, now_millis)
                .await
            {
                Ok(active) => active,
                Err(write) => {
                    let write = *write;
                    return Err(failure(
                        Self {
                            active: write.state,
                            command,
                        },
                        CloseError::Write(write.error),
                    ));
                }
            };
        }
        let receipt = match WorkerZeroProof::new(active.turn.driver.runtime_owner().clone(), token)
        {
            Ok(receipt) => receipt,
            Err(error) => {
                return Err(failure(Self { active, command }, error));
            }
        };
        let handoff = match active.turn.driver.take_zero_process_handoff() {
            Ok(handoff) => handoff,
            Err(error) => {
                return Err(failure(Self { active, command }, CloseError::Driver(error)));
            }
        };
        let ZeroProcessHandoff { lease, proof, .. } = handoff;
        Ok(ZeroCloseState {
            active,
            lease,
            proof: Some(CloseProof::Native(proof)),
            command,
            receipt,
        })
    }
}

impl<A, J, R, T, C> ZeroCloseState<A, J, R, T, C>
where
    A: HarnessAdapter,
    J: AdapterBatchJournal,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
    C::ZeroProof: CoordinatorZeroToken,
{
    pub(crate) fn receipt(&self) -> &WorkerZeroProof {
        &self.receipt
    }

    pub(crate) async fn finalize(
        mut self,
        command: &CloseSessionCommand,
        receipt: &WorkerZeroProof,
    ) -> Result<PreparedProcessWrite<A, J, R, T, C>, Box<CloseFailure<Self>>> {
        if command != &self.command || receipt != &self.receipt {
            return Err(failure(self, CloseError::Intent));
        }
        let retained = match self.proof.take() {
            Some(proof) => proof,
            None => return Err(failure(self, CloseError::Proof)),
        };
        let proof = match retained {
            CloseProof::Validated(proof) => proof,
            CloseProof::Native(native) => {
                match native.try_into_coordinator_proof(self.lease.owner().clone()) {
                    Ok(proof) => proof,
                    Err((native, error)) => {
                        self.proof = Some(CloseProof::Native(native));
                        return Err(failure(self, CloseError::Driver(error)));
                    }
                }
            }
        };
        match self
            .lease
            .finalize_close_after_zero_process_retained(
                FinalizeCloseCommand {
                    request: command.clone(),
                },
                proof,
            )
            .await
        {
            Ok(_) => Ok(self.active),
            Err((error, proof)) => {
                self.proof = Some(CloseProof::Validated(proof));
                Err(failure(self, error.into()))
            }
        }
    }
}

fn failure<S>(state: S, error: CloseError) -> Box<CloseFailure<S>> {
    Box::new(CloseFailure { state, error })
}
