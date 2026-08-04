use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    process::TreeExit,
    runtime::{
        ActivatedSession, ActiveDispatch, CoordinatorActivationHandoff,
        CoordinatorActivationJournal, IdleActivationParts, SessionTreeLease, ZeroProcessProof,
    },
    store::{CloseSessionCommand, FinalizeCloseCommand, SendObservation},
};

use super::{
    CompletedActivationDriverParts, CoordinatorZeroToken, PreparedActivationTurn,
    ProcessTreeControl, StdinTransport, WorkerZeroProof,
};

#[derive(Debug, thiserror::Error)]
pub(crate) enum IdleCloseError {
    #[error("the idle close command is stale or foreign")]
    Intent,
    #[error("the activation handoff is not ready for idle close")]
    Activation,
    #[error("the durable session close transition failed")]
    Session(#[from] crate::acp::runtime::SessionCoordinatorError),
    #[error("the prepared Turn close transition failed")]
    Turn(#[from] crate::acp::runtime::CoordinatorError),
    #[error("the process tree close transition failed")]
    Driver,
    #[error("the zero-process proof token is invalid or exhausted")]
    Proof,
}

pub(crate) struct IdleCloseFailure<S> {
    pub(crate) state: S,
    pub(crate) error: IdleCloseError,
}

pub(crate) enum IdleCloseBeginState<A, R, T, C>
where
    C: ProcessTreeControl,
{
    Activation(CompletedActivationDriverParts<A, CoordinatorActivationJournal, R, T, C>),
    Prepared(PreparedActivationTurn<A, R, T, C>),
    Candidate(IdleCloseCandidate<A, R, T, C>),
}

pub(crate) struct IdleCloseCandidate<A, R, T, C>
where
    C: ProcessTreeControl,
{
    _adapter: A,
    _resources: R,
    _session: ActivatedSession,
    transport: T,
    control: C,
    lease: SessionTreeLease,
    dispatch: Option<ActiveDispatch>,
    command: CloseSessionCommand,
    close_committed: bool,
    stdin_closed: bool,
}

pub(crate) struct IdleClosingState<A, R, T, C>
where
    C: ProcessTreeControl,
{
    candidate: IdleCloseCandidate<A, R, T, C>,
}

pub(crate) struct IdleZeroState<A, R, T, C>
where
    C: ProcessTreeControl,
    C::ZeroProof: CoordinatorZeroToken,
{
    candidate: IdleCloseCandidate<A, R, T, C>,
    proof: Option<IdleProof<C::ZeroProof>>,
    receipt: WorkerZeroProof,
}

enum IdleProof<P> {
    Native(P),
    Validated(ZeroProcessProof),
}

impl<A, R, T, C> IdleCloseBeginState<A, R, T, C>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
{
    pub(crate) fn activation(
        parts: CompletedActivationDriverParts<A, CoordinatorActivationJournal, R, T, C>,
    ) -> Self {
        Self::Activation(parts)
    }

    pub(crate) fn prepared(turn: PreparedActivationTurn<A, R, T, C>) -> Self {
        Self::Prepared(turn)
    }

    pub(crate) async fn begin(
        self,
        command: CloseSessionCommand,
    ) -> Result<IdleClosingState<A, R, T, C>, Box<IdleCloseFailure<Self>>> {
        let mut candidate = match self {
            Self::Candidate(candidate) => {
                if candidate.command != command {
                    return Err(failure(Self::Candidate(candidate), IdleCloseError::Intent));
                }
                candidate
            }
            source @ (Self::Activation(_) | Self::Prepared(_)) => {
                match candidate_from_source(source, command) {
                    Ok(candidate) => candidate,
                    Err(failure) => return Err(failure),
                }
            }
        };
        if !candidate.close_committed {
            if let Err(error) = candidate
                .lease
                .close_session(candidate.command.clone())
                .await
            {
                return Err(failure(Self::Candidate(candidate), error.into()));
            }
            candidate.close_committed = true;
        }
        if let Some(dispatch) = candidate.dispatch.as_mut() {
            if dispatch.projection_retry_pending()
                && let Err(error) = dispatch.retry_projection()
            {
                return Err(failure(Self::Candidate(candidate), error.into()));
            }
            if !dispatch.is_terminal()
                && let Err(error) = dispatch
                    .observe(SendObservation::ProcessLost, candidate.command.now_millis)
                    .await
            {
                return Err(failure(Self::Candidate(candidate), error.into()));
            }
        }
        if !candidate.stdin_closed {
            candidate.transport.close();
            candidate.stdin_closed = true;
        }
        if candidate
            .control
            .request_termination(TreeExit::Cancelled)
            .is_err()
        {
            return Err(failure(Self::Candidate(candidate), IdleCloseError::Driver));
        }
        Ok(IdleClosingState { candidate })
    }
}

impl<A, R, T, C> IdleClosingState<A, R, T, C>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
    C::ZeroProof: CoordinatorZeroToken,
{
    pub(crate) fn owner(&self) -> &crate::acp::scheduler::RuntimeOwner {
        self.candidate.lease.owner()
    }

    pub(crate) fn converge_zero(
        self,
        token: u64,
    ) -> Result<IdleZeroState<A, R, T, C>, Box<IdleCloseFailure<Self>>> {
        let receipt = match WorkerZeroProof::new(self.owner().clone(), token) {
            Ok(receipt) => receipt,
            Err(_) => return Err(failure(self, IdleCloseError::Proof)),
        };
        let proof = match self.candidate.control.wait_zero() {
            Ok((_exit, proof)) => proof,
            Err(_) => return Err(failure(self, IdleCloseError::Driver)),
        };
        Ok(IdleZeroState {
            candidate: self.candidate,
            proof: Some(IdleProof::Native(proof)),
            receipt,
        })
    }
}

impl<A, R, T, C> IdleZeroState<A, R, T, C>
where
    A: HarnessAdapter,
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
    ) -> Result<(), Box<IdleCloseFailure<Self>>> {
        if command != &self.candidate.command || receipt != &self.receipt {
            return Err(failure(self, IdleCloseError::Intent));
        }
        let retained = match self.proof.take() {
            Some(proof) => proof,
            None => return Err(failure(self, IdleCloseError::Proof)),
        };
        let proof = match retained {
            IdleProof::Validated(proof) => proof,
            IdleProof::Native(native) => {
                match native.try_into_coordinator_proof(self.candidate.lease.owner().clone()) {
                    Ok(proof) => proof,
                    Err((native, _)) => {
                        self.proof = Some(IdleProof::Native(native));
                        return Err(failure(self, IdleCloseError::Driver));
                    }
                }
            }
        };
        match self
            .candidate
            .lease
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
                self.proof = Some(IdleProof::Validated(proof));
                Err(failure(self, error.into()))
            }
        }
    }
}

#[allow(clippy::type_complexity)]
fn candidate_from_source<A, R, T, C>(
    source: IdleCloseBeginState<A, R, T, C>,
    command: CloseSessionCommand,
) -> Result<IdleCloseCandidate<A, R, T, C>, Box<IdleCloseFailure<IdleCloseBeginState<A, R, T, C>>>>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
{
    let (parts, dispatch, active_turn) = match source {
        IdleCloseBeginState::Activation(parts) => (parts, None, None),
        IdleCloseBeginState::Prepared(prepared) => {
            let PreparedActivationTurn {
                parts,
                dispatch,
                active_turn,
            } = prepared;
            (parts, Some(dispatch), Some(active_turn))
        }
        IdleCloseBeginState::Candidate(candidate) => return Ok(candidate),
    };
    if parts.owner.project_id != command.project_id
        || parts.owner.chat_id != command.chat_id
        || parts.owner.session_id != command.session_id
        || parts.owner.runtime_epoch != command.runtime_epoch
    {
        return Err(failure(
            rebuild_source(parts, dispatch, active_turn),
            IdleCloseError::Intent,
        ));
    }
    let CompletedActivationDriverParts {
        parts,
        transport,
        control,
        owner,
        clock,
    } = parts;
    let (adapter, journal, resources, session) = parts.into_parts();
    let (lease, handoff) = match journal.into_handoff() {
        Ok(value) => value,
        Err(journal) => {
            let parts = CompletedActivationDriverParts {
                parts: IdleActivationParts::new(adapter, *journal, resources, session),
                transport,
                control,
                owner,
                clock,
            };
            return Err(failure(
                rebuild_source(parts, dispatch, active_turn),
                IdleCloseError::Activation,
            ));
        }
    };
    if handoff != CoordinatorActivationHandoff::Ready {
        return Err(failure(
            IdleCloseBeginState::Candidate(IdleCloseCandidate {
                _adapter: adapter,
                _resources: resources,
                _session: session,
                transport,
                control,
                lease,
                dispatch,
                command,
                close_committed: false,
                stdin_closed: false,
            }),
            IdleCloseError::Activation,
        ));
    }
    Ok(IdleCloseCandidate {
        _adapter: adapter,
        _resources: resources,
        _session: session,
        transport,
        control,
        lease,
        dispatch,
        command,
        close_committed: false,
        stdin_closed: false,
    })
}

fn rebuild_source<A, R, T, C>(
    parts: CompletedActivationDriverParts<A, CoordinatorActivationJournal, R, T, C>,
    dispatch: Option<ActiveDispatch>,
    active_turn: Option<crate::acp::runtime::ActiveTurnIntent>,
) -> IdleCloseBeginState<A, R, T, C>
where
    C: ProcessTreeControl,
{
    match (dispatch, active_turn) {
        (Some(dispatch), Some(active_turn)) => {
            IdleCloseBeginState::Prepared(PreparedActivationTurn {
                parts,
                dispatch,
                active_turn,
            })
        }
        _ => IdleCloseBeginState::Activation(parts),
    }
}

fn failure<S>(state: S, error: IdleCloseError) -> Box<IdleCloseFailure<S>> {
    Box::new(IdleCloseFailure { state, error })
}
