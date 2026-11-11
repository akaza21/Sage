use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    runtime::{HostError, ProjectIntentAuthority, SqliteAdapterBatchJournal},
};

use super::super::{
    ActiveCloseBeginState, CloseError, CloseFailure, ClosingProcessWrite, CoordinatorZeroToken,
    ProcessTreeControl, StdinTransport, WorkerCommand, WorkerEnvelope, WorkerReply, ZeroCloseState,
};
use super::{FirstTurnWorkerContext, FirstTurnWorkerRecovery, FirstTurnWorkerState};

pub(super) async fn handle_begin<A, R, T, C, Auth>(
    state: ActiveCloseBeginState<A, SqliteAdapterBatchJournal, R, T, C>,
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
    if !matches!(
        &envelope.command,
        WorkerCommand::BeginClose(command) if command == state.command()
    ) {
        envelope.reject(HostError::RecoveryRequired);
        return FirstTurnWorkerState::Recovery(Box::new(FirstTurnWorkerRecovery::BeginClose(
            Box::new(CloseFailure {
                state,
                error: CloseError::Intent,
            }),
        )));
    }
    begin(state, envelope).await
}

pub(super) async fn begin<A, R, T, C>(
    state: ActiveCloseBeginState<A, SqliteAdapterBatchJournal, R, T, C>,
    envelope: WorkerEnvelope,
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
            envelope.respond(Ok(WorkerReply::Unit));
            FirstTurnWorkerState::Draining(Box::new(closing))
        }
        Err(failure) => {
            envelope.respond(Err(super::map_close(&failure.error)));
            FirstTurnWorkerState::Recovery(Box::new(FirstTurnWorkerRecovery::BeginClose(failure)))
        }
    }
}

pub(super) async fn handle_draining<A, R, T, C, Auth>(
    closing: Box<ClosingProcessWrite<A, SqliteAdapterBatchJournal, R, T, C>>,
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
        return FirstTurnWorkerState::Draining(closing);
    };
    if owner != &context.owner {
        envelope.reject(HostError::Intent);
        return FirstTurnWorkerState::Draining(closing);
    }
    let Some(token) = context.next_zero_token() else {
        envelope.reject(HostError::RecoveryRequired);
        return FirstTurnWorkerState::Draining(closing);
    };
    match closing
        .converge_zero(token, context.clock.now_millis())
        .await
    {
        Ok(zero) => {
            let proof = zero.receipt().clone();
            envelope.respond(Ok(WorkerReply::Zero(proof)));
            FirstTurnWorkerState::Zero(Box::new(zero))
        }
        Err(failure) => {
            envelope.respond(Err(super::map_close(&failure.error)));
            FirstTurnWorkerState::Recovery(Box::new(FirstTurnWorkerRecovery::Converge(failure)))
        }
    }
}

pub(super) async fn handle_zero<A, R, T, C, Auth>(
    zero: Box<ZeroCloseState<A, SqliteAdapterBatchJournal, R, T, C>>,
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
        return FirstTurnWorkerState::Zero(zero);
    };
    match zero.finalize(command, proof).await {
        Ok(closed) => {
            drop(closed);
            envelope.respond(Ok(WorkerReply::Unit));
            FirstTurnWorkerState::Closed
        }
        Err(failure) => {
            envelope.respond(Err(super::map_close(&failure.error)));
            FirstTurnWorkerState::Recovery(Box::new(FirstTurnWorkerRecovery::Finalize(failure)))
        }
    }
}
