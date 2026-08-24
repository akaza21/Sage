use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    runtime::{HostError, ProjectIntentAuthority, SqliteAdapterBatchJournal},
};

use super::super::{
    CloseError, CloseFailure, CoordinatorZeroToken, ProcessTreeControl, ReusableCloseBeginState,
    ReusableClosingState, ReusableZeroState, StdinTransport, WorkerCommand, WorkerEnvelope,
    WorkerReply,
};
use super::{FirstTurnWorkerContext, FirstTurnWorkerRecovery, FirstTurnWorkerState};

pub(super) async fn handle_begin<A, R, T, C, Auth>(
    state: ReusableCloseBeginState<A, SqliteAdapterBatchJournal, R, T, C>,
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
    if !matches!(&command, WorkerCommand::BeginClose(close) if close == state.command()) {
        responder.respond(Err(HostError::RecoveryRequired));
        return FirstTurnWorkerState::Recovery(Box::new(FirstTurnWorkerRecovery::ReusableBegin(
            Box::new(CloseFailure {
                state,
                error: CloseError::Intent,
            }),
        )));
    }
    begin(state, responder).await
}

pub(super) async fn begin<A, R, T, C>(
    state: ReusableCloseBeginState<A, SqliteAdapterBatchJournal, R, T, C>,
    responder: super::super::worker_command::WorkerResponder,
) -> FirstTurnWorkerState<A, R, T, C>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
    C::ZeroProof: CoordinatorZeroToken,
{
    match state.begin().await {
        Ok(closing) => {
            responder.respond(Ok(WorkerReply::Unit));
            FirstTurnWorkerState::ReusableDraining(closing)
        }
        Err(failure) => {
            responder.respond(Err(super::map_close(&failure.error)));
            FirstTurnWorkerState::Recovery(Box::new(FirstTurnWorkerRecovery::ReusableBegin(
                failure,
            )))
        }
    }
}

pub(super) async fn handle_draining<A, R, T, C, Auth>(
    closing: ReusableClosingState<A, SqliteAdapterBatchJournal, R, T, C>,
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
        return FirstTurnWorkerState::ReusableDraining(closing);
    };
    if owner != closing.owner() {
        envelope.reject(HostError::Intent);
        return FirstTurnWorkerState::ReusableDraining(closing);
    }
    let Some(token) = context.next_zero_token() else {
        envelope.reject(HostError::RecoveryRequired);
        return FirstTurnWorkerState::ReusableDraining(closing);
    };
    match closing.converge_zero(token) {
        Ok(zero) => {
            envelope.respond(Ok(WorkerReply::Zero(zero.receipt().clone())));
            FirstTurnWorkerState::ReusableZero(zero)
        }
        Err(failure) => {
            envelope.respond(Err(super::map_close(&failure.error)));
            FirstTurnWorkerState::Recovery(Box::new(FirstTurnWorkerRecovery::ReusableConverge(
                failure,
            )))
        }
    }
}

pub(super) async fn handle_zero<A, R, T, C, Auth>(
    zero: ReusableZeroState<A, SqliteAdapterBatchJournal, R, T, C>,
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
    match &envelope.command {
        WorkerCommand::ParkAfterZero { owner } => {
            if owner != zero.owner() {
                envelope.reject(HostError::Intent);
                return FirstTurnWorkerState::ReusableZero(zero);
            }
            match zero.park().await {
                Ok(()) => {
                    envelope.respond(Ok(WorkerReply::Unit));
                    FirstTurnWorkerState::Closed
                }
                Err(failure) => {
                    envelope.respond(Err(super::map_close(&failure.error)));
                    FirstTurnWorkerState::Recovery(Box::new(
                        FirstTurnWorkerRecovery::ReusableFinalize(failure),
                    ))
                }
            }
        }
        WorkerCommand::FinalizeClose { command, proof } => {
            match zero.finalize(command, proof).await {
                Ok(()) => {
                    envelope.respond(Ok(WorkerReply::Unit));
                    FirstTurnWorkerState::Closed
                }
                Err(failure) => {
                    envelope.respond(Err(super::map_close(&failure.error)));
                    FirstTurnWorkerState::Recovery(Box::new(
                        FirstTurnWorkerRecovery::ReusableFinalize(failure),
                    ))
                }
            }
        }
        _ => {
            envelope.reject(HostError::State);
            FirstTurnWorkerState::ReusableZero(zero)
        }
    }
}
