use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    process::TreeExit,
    runtime::{AdapterBatchJournal, SessionTreeLease, ZeroProcessProof},
    store::{CloseSessionCommand, FinalizeCloseCommand},
};

use super::{
    CloseError, CloseFailure, CompletedDriverParts, CoordinatorZeroToken, DriverError,
    ProcessTreeControl, StdinTransport, WorkerZeroProof,
};

pub(crate) struct ReusableCloseBeginState<A, J, R, T, C>
where
    A: HarnessAdapter,
    J: AdapterBatchJournal,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
{
    parts: CompletedDriverParts<A, J, R, T, C, SessionTreeLease>,
    command: CloseSessionCommand,
    close_committed: bool,
    stdin_closed: bool,
}

pub(crate) struct ReusableClosingState<A, J, R, T, C>
where
    A: HarnessAdapter,
    J: AdapterBatchJournal,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
{
    candidate: ReusableCloseBeginState<A, J, R, T, C>,
}

pub(crate) struct ReusableZeroState<A, J, R, T, C>
where
    A: HarnessAdapter,
    J: AdapterBatchJournal,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
    C::ZeroProof: CoordinatorZeroToken,
{
    candidate: ReusableCloseBeginState<A, J, R, T, C>,
    proof: Option<ReusableProof<C::ZeroProof>>,
    receipt: WorkerZeroProof,
}

enum ReusableProof<P> {
    Native(P),
    Validated(ZeroProcessProof),
}

impl<A, J, R, T, C> ReusableCloseBeginState<A, J, R, T, C>
where
    A: HarnessAdapter,
    J: AdapterBatchJournal,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
{
    pub(crate) fn new(
        parts: CompletedDriverParts<A, J, R, T, C, SessionTreeLease>,
        command: CloseSessionCommand,
    ) -> Self {
        Self {
            parts,
            command,
            close_committed: false,
            stdin_closed: false,
        }
    }

    pub(crate) fn command(&self) -> &CloseSessionCommand {
        &self.command
    }

    pub(crate) async fn begin_park(
        parts: CompletedDriverParts<A, J, R, T, C, SessionTreeLease>,
    ) -> Result<ReusableClosingState<A, J, R, T, C>, Box<CloseFailure<Self>>> {
        let command = CloseSessionCommand {
            project_id: parts.owner.project_id.clone(),
            chat_id: parts.owner.chat_id.clone(),
            expected_projection_revision: crate::acp::contract::ProjectionRevision::ZERO,
            client_intent_id: crate::acp::contract::ClientIntentId::new(),
            session_id: parts.owner.session_id.clone(),
            runtime_epoch: parts.owner.runtime_epoch,
            now_millis: crate::persistence::now_millis(),
        };
        Self {
            parts,
            command,
            close_committed: true,
            stdin_closed: false,
        }
        .begin()
        .await
    }

    pub(crate) async fn begin(
        mut self,
    ) -> Result<ReusableClosingState<A, J, R, T, C>, Box<CloseFailure<Self>>> {
        if self.parts.owner.project_id != self.command.project_id
            || self.parts.owner.chat_id != self.command.chat_id
            || self.parts.owner.session_id != self.command.session_id
            || self.parts.owner.runtime_epoch != self.command.runtime_epoch
        {
            return Err(failure(self, CloseError::Intent));
        }
        if !self.close_committed {
            let close_result = match self.parts.lease.as_mut() {
                Some(lease) => lease.close_session(self.command.clone()).await,
                None => return Err(failure(self, CloseError::Driver(DriverError::Tree))),
            };
            if let Err(error) = close_result {
                return Err(failure(self, error.into()));
            }
            self.close_committed = true;
        }
        if !self.stdin_closed {
            self.parts.write_close_session();
            self.parts.transport.close();
            self.stdin_closed = true;
        }
        if self
            .parts
            .control
            .request_termination(TreeExit::Cancelled)
            .is_err()
        {
            return Err(failure(self, CloseError::Driver(DriverError::Tree)));
        }
        Ok(ReusableClosingState { candidate: self })
    }
}

impl<A, J, R, T, C> ReusableClosingState<A, J, R, T, C>
where
    A: HarnessAdapter,
    J: AdapterBatchJournal,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
    C::ZeroProof: CoordinatorZeroToken,
{
    pub(crate) fn owner(&self) -> &crate::acp::scheduler::RuntimeOwner {
        &self.candidate.parts.owner
    }

    #[allow(clippy::type_complexity)]
    pub(crate) fn converge_zero(
        self,
        token: u64,
    ) -> Result<ReusableZeroState<A, J, R, T, C>, Box<CloseFailure<Self>>> {
        let receipt = match WorkerZeroProof::new(self.owner().clone(), token) {
            Ok(receipt) => receipt,
            Err(error) => return Err(failure(self, error)),
        };
        let proof = match self.candidate.parts.control.wait_zero() {
            Ok((_exit, proof)) => proof,
            Err(_) => return Err(failure(self, CloseError::Driver(DriverError::Tree))),
        };
        Ok(ReusableZeroState {
            candidate: self.candidate,
            proof: Some(ReusableProof::Native(proof)),
            receipt,
        })
    }
}

impl<A, J, R, T, C> ReusableZeroState<A, J, R, T, C>
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

    pub(crate) fn owner(&self) -> &crate::acp::scheduler::RuntimeOwner {
        &self.candidate.parts.owner
    }

    pub(crate) async fn park(mut self) -> Result<(), Box<CloseFailure<Self>>> {
        let retained = match self.proof.take() {
            Some(proof) => proof,
            None => return Err(failure(self, CloseError::Proof)),
        };
        let owner = self.candidate.parts.owner.clone();
        let proof = match retained {
            ReusableProof::Validated(proof) => proof,
            ReusableProof::Native(native) => match native.try_into_coordinator_proof(owner.clone())
            {
                Ok(proof) => proof,
                Err((native, error)) => {
                    self.proof = Some(ReusableProof::Native(native));
                    return Err(failure(self, CloseError::Driver(error)));
                }
            },
        };
        if !matches!(
            self.candidate.parts.terminal_cursor().state(),
            crate::acp::lifecycle::TurnState::Completed
        ) {
            return Ok(());
        }
        let Some(lease) = self.candidate.parts.lease.as_mut() else {
            self.proof = Some(ReusableProof::Validated(proof));
            return Err(failure(self, CloseError::Driver(DriverError::Tree)));
        };
        let Ok(exit_kind) = crate::acp::contract::RequiredText::new("exitKind", "hostShutdown")
        else {
            self.proof = Some(ReusableProof::Validated(proof));
            return Err(failure(self, CloseError::Proof));
        };
        match lease
            .park_after_zero_process(
                crate::acp::store::ParkSessionCommand {
                    project_id: owner.project_id,
                    chat_id: owner.chat_id,
                    session_id: owner.session_id,
                    runtime_epoch: owner.runtime_epoch,
                    exit_kind,
                    now_millis: crate::persistence::now_millis(),
                },
                proof,
            )
            .await
        {
            Ok(_) => Ok(()),
            Err(error) => Err(failure(self, error.into())),
        }
    }

    pub(crate) async fn finalize(
        mut self,
        command: &CloseSessionCommand,
        receipt: &WorkerZeroProof,
    ) -> Result<(), Box<CloseFailure<Self>>> {
        if command != &self.candidate.command || receipt != &self.receipt {
            return Err(failure(self, CloseError::Intent));
        }
        let retained = match self.proof.take() {
            Some(proof) => proof,
            None => return Err(failure(self, CloseError::Proof)),
        };
        let owner = self.candidate.parts.owner.clone();
        let proof = match retained {
            ReusableProof::Validated(proof) => proof,
            ReusableProof::Native(native) => match native.try_into_coordinator_proof(owner) {
                Ok(proof) => proof,
                Err((native, error)) => {
                    self.proof = Some(ReusableProof::Native(native));
                    return Err(failure(self, CloseError::Driver(error)));
                }
            },
        };
        let Some(lease) = self.candidate.parts.lease.as_mut() else {
            self.proof = Some(ReusableProof::Validated(proof));
            return Err(failure(self, CloseError::Driver(DriverError::Tree)));
        };
        match lease
            .finalize_close_after_zero_process_retained(
                FinalizeCloseCommand {
                    request: command.clone(),
                },
                proof,
            )
            .await
        {
            Ok(_) => Ok(()),
            Err((error, proof)) => {
                self.proof = Some(ReusableProof::Validated(proof));
                Err(failure(self, error.into()))
            }
        }
    }
}

fn failure<S>(state: S, error: CloseError) -> Box<CloseFailure<S>> {
    Box::new(CloseFailure { state, error })
}
