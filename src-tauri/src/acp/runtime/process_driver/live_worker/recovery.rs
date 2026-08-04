use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    runtime::{HostCancelIntent, HostError, SqliteAdapterBatchJournal},
};

use super::super::{
    CoordinatorZeroToken, PreparedProcessWrite, PreparedWriteError, ProcessTreeControl,
    StdinTransport, WorkerCommand, WorkerEnvelope, WorkerReply,
};
use super::turn::map_write;
use super::write_recovery::{
    classify_begin_send_failure, classify_observe_failure, durable_commit_unknown,
};
use super::{FirstTurnWorkerContext, FirstTurnWorkerRecovery, FirstTurnWorkerState};

pub(super) fn handle_begin_send<A, R, T, C>(
    active: PreparedProcessWrite<A, SqliteAdapterBatchJournal, R, T, C>,
    owner: crate::acp::scheduler::RuntimeOwner,
    turn_id: crate::acp::contract::SageTurnId,
    envelope: WorkerEnvelope,
) -> FirstTurnWorkerState<A, R, T, C>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
    C::ZeroProof: CoordinatorZeroToken,
{
    if !matches!(
        &envelope.command,
        WorkerCommand::DispatchPrompt {
            owner: candidate_owner,
            turn_id: candidate_turn,
        } if candidate_owner == &owner && candidate_turn == &turn_id
    ) {
        envelope.reject(HostError::RecoveryRequired);
        return FirstTurnWorkerState::Recovery(Box::new(FirstTurnWorkerRecovery::BeginSend {
            failure: Box::new(super::super::PreparedWriteFailure {
                state: active,
                error: PreparedWriteError::Intent,
            }),
            owner,
            turn_id,
        }));
    }
    match active.retry_send_projection() {
        Ok((active, write)) => {
            envelope.respond(Ok(WorkerReply::Write(write)));
            FirstTurnWorkerState::Active(active)
        }
        Err(failure) => {
            envelope.respond(Err(map_write(&failure.error)));
            FirstTurnWorkerState::Recovery(Box::new(FirstTurnWorkerRecovery::BeginSend {
                failure,
                owner,
                turn_id,
            }))
        }
    }
}

pub(super) async fn handle_begin_send_retry<A, R, T, C, Auth>(
    active: PreparedProcessWrite<A, SqliteAdapterBatchJournal, R, T, C>,
    owner: crate::acp::scheduler::RuntimeOwner,
    turn_id: crate::acp::contract::SageTurnId,
    context: &FirstTurnWorkerContext<Auth>,
    envelope: WorkerEnvelope,
) -> FirstTurnWorkerState<A, R, T, C>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
    C::ZeroProof: CoordinatorZeroToken,
{
    if !matches!(
        &envelope.command,
        WorkerCommand::DispatchPrompt {
            owner: candidate_owner,
            turn_id: candidate_turn,
        } if candidate_owner == &owner && candidate_turn == &turn_id
    ) {
        envelope.reject(HostError::RecoveryRequired);
        return FirstTurnWorkerState::Recovery(Box::new(FirstTurnWorkerRecovery::BeginSendRetry {
            failure: Box::new(super::super::PreparedWriteFailure {
                state: active,
                error: PreparedWriteError::Intent,
            }),
            owner,
            turn_id,
        }));
    }
    match active.dispatch_once(context.clock.now_millis()).await {
        Ok((active, write)) => {
            envelope.respond(Ok(WorkerReply::Write(write)));
            FirstTurnWorkerState::Active(active)
        }
        Err(failure) => classify_begin_send_failure(failure, owner, turn_id, envelope),
    }
}

pub(super) fn handle_terminal<A, R, T, C, Auth>(
    active: PreparedProcessWrite<A, SqliteAdapterBatchJournal, R, T, C>,
    owner: crate::acp::scheduler::RuntimeOwner,
    cursor: crate::acp::store::AdapterTurnCursor,
    context: &FirstTurnWorkerContext<Auth>,
    envelope: WorkerEnvelope,
) -> FirstTurnWorkerState<A, R, T, C>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
    C::ZeroProof: CoordinatorZeroToken,
{
    if !matches!(
        &envelope.command,
        WorkerCommand::AcceptTerminal {
            owner: candidate_owner,
            cursor: candidate_cursor,
        } if candidate_owner == &owner && candidate_cursor == &cursor
    ) {
        envelope.reject(HostError::RecoveryRequired);
        return FirstTurnWorkerState::Recovery(Box::new(FirstTurnWorkerRecovery::Terminal {
            failure: Box::new(super::super::PreparedWriteFailure {
                state: active,
                error: PreparedWriteError::Intent,
            }),
            owner,
            cursor,
        }));
    }
    accept_terminal_once(active, owner, cursor, context, envelope)
}

pub(super) fn accept_terminal_once<A, R, T, C, Auth>(
    active: PreparedProcessWrite<A, SqliteAdapterBatchJournal, R, T, C>,
    owner: crate::acp::scheduler::RuntimeOwner,
    cursor: crate::acp::store::AdapterTurnCursor,
    context: &FirstTurnWorkerContext<Auth>,
    envelope: WorkerEnvelope,
) -> FirstTurnWorkerState<A, R, T, C>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
    C::ZeroProof: CoordinatorZeroToken,
{
    let active = match active.accept_terminal(&cursor) {
        Ok(active) => active,
        Err(failure)
            if matches!(
                failure.error,
                PreparedWriteError::Intent
                    | PreparedWriteError::Coordinator(
                        crate::acp::runtime::CoordinatorError::AdapterCursorForeign
                            | crate::acp::runtime::CoordinatorError::AdapterCursorStale
                    )
            ) =>
        {
            envelope.respond(Err(HostError::Intent));
            return FirstTurnWorkerState::Active(failure.state);
        }
        Err(failure) => {
            envelope.respond(Err(map_write(&failure.error)));
            return FirstTurnWorkerState::Recovery(Box::new(FirstTurnWorkerRecovery::Terminal {
                failure,
                owner,
                cursor,
            }));
        }
    };
    match active.try_into_completed(context.mailbox.as_ref()) {
        Ok(completed) => {
            envelope.respond(Ok(WorkerReply::Unit));
            FirstTurnWorkerState::Reusable(completed)
        }
        Err(failure) => {
            envelope.respond(Err(map_write(&failure.error)));
            FirstTurnWorkerState::Recovery(Box::new(FirstTurnWorkerRecovery::Terminal {
                failure,
                owner,
                cursor,
            }))
        }
    }
}

pub(super) fn handle_observe<A, R, T, C>(
    active: PreparedProcessWrite<A, SqliteAdapterBatchJournal, R, T, C>,
    owner: crate::acp::scheduler::RuntimeOwner,
    turn_id: crate::acp::contract::SageTurnId,
    observation: crate::acp::store::SendObservation,
    envelope: WorkerEnvelope,
) -> FirstTurnWorkerState<A, R, T, C>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
    C::ZeroProof: CoordinatorZeroToken,
{
    if !matches!(
        &envelope.command,
        WorkerCommand::ObserveSend {
            owner: candidate_owner,
            turn_id: candidate_turn,
            observation: candidate_observation,
        } if candidate_owner == &owner
            && candidate_turn == &turn_id
            && candidate_observation == &observation
    ) {
        envelope.reject(HostError::RecoveryRequired);
        return FirstTurnWorkerState::Recovery(Box::new(FirstTurnWorkerRecovery::Observe {
            failure: Box::new(super::super::PreparedWriteFailure {
                state: active,
                error: PreparedWriteError::Intent,
            }),
            owner,
            turn_id,
            observation,
        }));
    }
    match active.retry_observation_projection(observation) {
        Ok(active) => {
            envelope.respond(Ok(WorkerReply::Unit));
            FirstTurnWorkerState::Active(active)
        }
        Err(failure) => {
            envelope.respond(Err(map_write(&failure.error)));
            FirstTurnWorkerState::Recovery(Box::new(FirstTurnWorkerRecovery::Observe {
                failure,
                owner,
                turn_id,
                observation,
            }))
        }
    }
}

pub(super) async fn handle_observe_retry<A, R, T, C, Auth>(
    active: PreparedProcessWrite<A, SqliteAdapterBatchJournal, R, T, C>,
    owner: crate::acp::scheduler::RuntimeOwner,
    turn_id: crate::acp::contract::SageTurnId,
    observation: crate::acp::store::SendObservation,
    context: &FirstTurnWorkerContext<Auth>,
    envelope: WorkerEnvelope,
) -> FirstTurnWorkerState<A, R, T, C>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
    C::ZeroProof: CoordinatorZeroToken,
{
    if !matches!(
        &envelope.command,
        WorkerCommand::ObserveSend {
            owner: candidate_owner,
            turn_id: candidate_turn,
            observation: candidate_observation,
        } if candidate_owner == &owner
            && candidate_turn == &turn_id
            && candidate_observation == &observation
    ) {
        envelope.reject(HostError::RecoveryRequired);
        return FirstTurnWorkerState::Recovery(Box::new(FirstTurnWorkerRecovery::ObserveRetry {
            failure: Box::new(super::super::PreparedWriteFailure {
                state: active,
                error: PreparedWriteError::Intent,
            }),
            owner,
            turn_id,
            observation,
        }));
    }
    match active
        .observe(observation, context.clock.now_millis())
        .await
    {
        Ok(active) => {
            envelope.respond(Ok(WorkerReply::Unit));
            FirstTurnWorkerState::Active(active)
        }
        Err(failure) => classify_observe_failure(failure, owner, turn_id, observation, envelope),
    }
}

pub(super) async fn handle_begin_cancel<A, R, T, C>(
    active: PreparedProcessWrite<A, SqliteAdapterBatchJournal, R, T, C>,
    intent: HostCancelIntent,
    envelope: WorkerEnvelope,
) -> FirstTurnWorkerState<A, R, T, C>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
    C::ZeroProof: CoordinatorZeroToken,
{
    if !matches!(&envelope.command, WorkerCommand::BeginCancel(candidate) if candidate == &intent) {
        envelope.reject(HostError::RecoveryRequired);
        return FirstTurnWorkerState::Recovery(Box::new(FirstTurnWorkerRecovery::BeginCancel {
            failure: Box::new(super::super::PreparedWriteFailure {
                state: active,
                error: PreparedWriteError::Intent,
            }),
            intent,
        }));
    }
    match active.begin_cancel(&intent).await {
        Ok(active) => {
            envelope.respond(Ok(WorkerReply::Unit));
            FirstTurnWorkerState::Active(active)
        }
        Err(failure) if durable_commit_unknown(&failure.error) => {
            envelope.respond(Err(map_write(&failure.error)));
            FirstTurnWorkerState::Active(failure.state.mark_durable_mutation_unknown())
        }
        Err(failure) => {
            envelope.respond(Err(map_write(&failure.error)));
            FirstTurnWorkerState::Recovery(Box::new(FirstTurnWorkerRecovery::BeginCancel {
                failure,
                intent,
            }))
        }
    }
}
