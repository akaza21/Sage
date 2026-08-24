use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    runtime::{HostError, SqliteAdapterBatchJournal},
};

use super::super::{
    CoordinatorZeroToken, PreparedProcessWrite, ProcessTreeControl, StdinTransport, WorkerCommand,
    WorkerEnvelope, WorkerReply, WorkerZeroProof,
};
use super::{FirstTurnWorkerContext, FirstTurnWorkerState};

pub(super) fn accept_latest_completed<A, R, T, C, Auth>(
    active: PreparedProcessWrite<A, SqliteAdapterBatchJournal, R, T, C>,
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
    let Some(cursor) = active
        .turn
        .driver
        .latest_durable_turn_cursor()
        .filter(|cursor| cursor.state() == crate::acp::lifecycle::TurnState::Completed)
        .cloned()
    else {
        envelope.reject(HostError::State);
        return FirstTurnWorkerState::Active(active);
    };
    let owner = context.owner.clone();
    super::recovery::accept_terminal_once(active, owner, cursor, context, envelope)
}

pub(super) async fn wait_zero_active<A, R, T, C, Auth>(
    mut active: PreparedProcessWrite<A, SqliteAdapterBatchJournal, R, T, C>,
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
    let WorkerCommand::WaitZero { owner } = &envelope.command else {
        envelope.reject(HostError::Intent);
        return FirstTurnWorkerState::Active(active);
    };
    if owner != &context.owner || !active.has_durable_terminal() {
        envelope.reject(HostError::Intent);
        return FirstTurnWorkerState::Active(active);
    }
    let Some(token) = context.next_zero_token() else {
        envelope.reject(HostError::RecoveryRequired);
        return FirstTurnWorkerState::Active(active);
    };
    let receipt = match WorkerZeroProof::new(context.owner.clone(), token) {
        Ok(receipt) => receipt,
        Err(_) => {
            envelope.reject(HostError::RecoveryRequired);
            return FirstTurnWorkerState::Active(active);
        }
    };
    match active.turn.driver.control.wait_zero() {
        Ok((_exit, native)) => {
            if active
                .turn
                .driver
                .latest_durable_turn_cursor()
                .is_some_and(|cursor| cursor.state() == crate::acp::lifecycle::TurnState::Completed)
                && let Ok(proof) = native.try_into_coordinator_proof(context.owner.clone())
                && let Some(lease) = active.turn.driver.lease.as_mut()
                && let Ok(exit_kind) =
                    crate::acp::contract::RequiredText::new("exitKind", "hostShutdown")
            {
                let _ = lease
                    .park_after_zero_process(
                        crate::acp::store::ParkSessionCommand {
                            project_id: context.owner.project_id.clone(),
                            chat_id: context.owner.chat_id.clone(),
                            session_id: context.owner.session_id.clone(),
                            runtime_epoch: context.owner.runtime_epoch,
                            exit_kind,
                            now_millis: crate::persistence::now_millis(),
                        },
                        proof,
                    )
                    .await;
            }
            envelope.respond(Ok(WorkerReply::Zero(receipt)));
            FirstTurnWorkerState::Active(active)
        }
        Err(_) => {
            envelope.reject(HostError::RecoveryRequired);
            FirstTurnWorkerState::Active(active)
        }
    }
}

pub(super) fn park_after_zero_active<A, R, T, C, Auth>(
    active: PreparedProcessWrite<A, SqliteAdapterBatchJournal, R, T, C>,
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
    let WorkerCommand::ParkAfterZero { owner } = &envelope.command else {
        envelope.reject(HostError::Intent);
        return FirstTurnWorkerState::Active(active);
    };
    if owner != &context.owner || !active.has_durable_terminal() {
        envelope.reject(HostError::Intent);
        return FirstTurnWorkerState::Active(active);
    }
    envelope.respond(Ok(WorkerReply::Unit));
    FirstTurnWorkerState::Closed
}
