use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    runtime::{HostError, ProjectIntentAuthority},
    store::CloseSessionCommand,
};

use super::super::{
    CoordinatorZeroToken, IdleCloseBeginState, IdleCloseError, IdleCloseFailure, IdleClosingState,
    IdleZeroState, ProcessTreeControl, StdinTransport, WorkerCommand, WorkerEnvelope, WorkerReply,
};
use super::{FirstTurnWorkerContext, FirstTurnWorkerRecovery, FirstTurnWorkerState};

pub(super) async fn handle_begin<A, R, T, C, Auth>(
    state: IdleCloseBeginState<A, R, T, C>,
    _context: &FirstTurnWorkerContext<Auth>,
    envelope: WorkerEnvelope,
) -> FirstTurnWorkerState<A, R, T, C>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
    C::ZeroProof: CoordinatorZeroToken,
    Auth: ProjectIntentAuthority,
{
    let (command, responder) = envelope.into_parts();
    let WorkerCommand::BeginClose(command) = command else {
        responder.respond(Err(HostError::RecoveryRequired));
        return FirstTurnWorkerState::Recovery(Box::new(FirstTurnWorkerRecovery::IdleBegin(
            Box::new(IdleCloseFailure {
                state,
                error: IdleCloseError::Intent,
            }),
        )));
    };
    begin(state, command, responder).await
}

pub(super) async fn begin<A, R, T, C>(
    state: IdleCloseBeginState<A, R, T, C>,
    command: CloseSessionCommand,
    responder: super::super::worker_command::WorkerResponder,
) -> FirstTurnWorkerState<A, R, T, C>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
    C::ZeroProof: CoordinatorZeroToken,
{
    match state.begin(command).await {
        Ok(closing) => {
            responder.respond(Ok(WorkerReply::Unit));
            FirstTurnWorkerState::IdleDraining(closing)
        }
        Err(failure) => {
            responder.respond(Err(map_error(&failure.error)));
            FirstTurnWorkerState::Recovery(Box::new(FirstTurnWorkerRecovery::IdleBegin(failure)))
        }
    }
}

pub(super) async fn handle_draining<A, R, T, C, Auth>(
    closing: IdleClosingState<A, R, T, C>,
    context: &FirstTurnWorkerContext<Auth>,
    envelope: WorkerEnvelope,
) -> FirstTurnWorkerState<A, R, T, C>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
    C::ZeroProof: CoordinatorZeroToken,
    Auth: ProjectIntentAuthority,
{
    let WorkerCommand::WaitZero { owner } = &envelope.command else {
        envelope.reject(HostError::State);
        return FirstTurnWorkerState::IdleDraining(closing);
    };
    if owner != closing.owner() {
        envelope.reject(HostError::Intent);
        return FirstTurnWorkerState::IdleDraining(closing);
    }
    let Some(token) = context.next_zero_token() else {
        envelope.reject(HostError::RecoveryRequired);
        return FirstTurnWorkerState::IdleDraining(closing);
    };
    match closing.converge_zero(token) {
        Ok(zero) => {
            envelope.respond(Ok(WorkerReply::Zero(zero.receipt().clone())));
            FirstTurnWorkerState::IdleZero(zero)
        }
        Err(failure) => {
            envelope.respond(Err(map_error(&failure.error)));
            FirstTurnWorkerState::Recovery(Box::new(FirstTurnWorkerRecovery::IdleConverge(failure)))
        }
    }
}

pub(super) async fn handle_zero<A, R, T, C, Auth>(
    zero: IdleZeroState<A, R, T, C>,
    _context: &FirstTurnWorkerContext<Auth>,
    envelope: WorkerEnvelope,
) -> FirstTurnWorkerState<A, R, T, C>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
    C::ZeroProof: CoordinatorZeroToken,
    Auth: ProjectIntentAuthority,
{
    let WorkerCommand::FinalizeClose { command, proof } = &envelope.command else {
        envelope.reject(HostError::State);
        return FirstTurnWorkerState::IdleZero(zero);
    };
    match zero.finalize(command, proof).await {
        Ok(()) => {
            envelope.respond(Ok(WorkerReply::Unit));
            FirstTurnWorkerState::Closed
        }
        Err(failure) => {
            envelope.respond(Err(map_error(&failure.error)));
            FirstTurnWorkerState::Recovery(Box::new(FirstTurnWorkerRecovery::IdleFinalize(failure)))
        }
    }
}

fn map_error(error: &IdleCloseError) -> HostError {
    match error {
        IdleCloseError::Intent => HostError::Intent,
        IdleCloseError::Activation
        | IdleCloseError::Session(_)
        | IdleCloseError::Turn(_)
        | IdleCloseError::Driver
        | IdleCloseError::Proof => HostError::RecoveryRequired,
    }
}
