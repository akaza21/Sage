use std::sync::Arc;

use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    runtime::{
        CoordinatorActivationJournal, HostError, ProjectIntentAuthority, SqliteAdapterBatchJournal,
    },
};

use super::super::{
    ActiveCloseBeginState, CompletedActivationDriverParts, CompletedDriverParts,
    CoordinatorZeroToken, IdleCloseBeginState, PrepareActivationTurnError, PrepareDriverTurnError,
    PreparedActivationTurn, PreparedProcessWrite, PreparedWriteError, ProcessTreeControl,
    ReusableCloseBeginState, StdinTransport, WorkerCommand, WorkerEnvelope, WorkerReply,
};
use super::write_recovery::{
    classify_begin_send_failure, classify_observe_failure, durable_commit_unknown,
    retain_active_after_write_error,
};
use super::{FirstTurnWorkerContext, FirstTurnWorkerRecovery, FirstTurnWorkerState};

pub(super) async fn handle_reusable<A, R, T, C, Auth>(
    parts: CompletedDriverParts<
        A,
        SqliteAdapterBatchJournal,
        R,
        T,
        C,
        crate::acp::runtime::SessionTreeLease,
    >,
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
    let (command, responder) = envelope.into_parts();
    match command {
        WorkerCommand::PreparePrompt(intent) => {
            if intent.owner() != context.owner {
                responder.respond(Err(HostError::Intent));
                return FirstTurnWorkerState::Reusable(parts);
            }
            let prompt = intent.command.prompt.clone();
            match parts
                .prepare_and_bind(&context.coordinator, intent.command)
                .await
            {
                Ok(turn) => {
                    let turn_id = turn.dispatch.turn_id().clone();
                    responder.respond(Ok(WorkerReply::Prepared(turn_id)));
                    FirstTurnWorkerState::Active(PreparedProcessWrite::new(turn, prompt))
                }
                Err(failure) => {
                    responder.respond(Err(map_prepare_next(&failure.error)));
                    FirstTurnWorkerState::Recovery(Box::new(FirstTurnWorkerRecovery::PrepareNext(
                        failure,
                    )))
                }
            }
        }
        WorkerCommand::BeginClose(command) => {
            super::reusable::begin(ReusableCloseBeginState::new(parts, command), responder).await
        }
        _ => {
            responder.respond(Err(HostError::State));
            FirstTurnWorkerState::Reusable(parts)
        }
    }
}

pub(super) async fn handle_idle<A, R, T, C, Auth>(
    parts: CompletedActivationDriverParts<A, CoordinatorActivationJournal, R, T, C>,
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
    let (command, responder) = envelope.into_parts();
    match command {
        WorkerCommand::PreparePrompt(intent) => {
            if intent.owner() != context.owner {
                responder.respond(Err(HostError::Intent));
                return FirstTurnWorkerState::Idle(parts);
            }
            let prompt = intent.command.prompt.clone();
            match parts
                .prepare_first_turn(&context.coordinator, intent.command)
                .await
            {
                Ok(turn) => {
                    let turn_id = turn.dispatch.turn_id().clone();
                    responder.respond(Ok(WorkerReply::Prepared(turn_id)));
                    FirstTurnWorkerState::Prepared { turn, prompt }
                }
                Err(failure) => {
                    responder.respond(Err(map_prepare(&failure.error)));
                    FirstTurnWorkerState::Recovery(Box::new(FirstTurnWorkerRecovery::Prepare(
                        failure,
                    )))
                }
            }
        }
        WorkerCommand::BeginClose(command) => {
            super::idle::begin(IdleCloseBeginState::activation(parts), command, responder).await
        }
        _ => {
            responder.respond(Err(HostError::State));
            FirstTurnWorkerState::Idle(parts)
        }
    }
}

pub(super) async fn handle_prepared<A, R, T, C, Auth>(
    turn: PreparedActivationTurn<A, R, T, C>,
    prompt: crate::acp::intent::Prompt,
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
    match &envelope.command {
        WorkerCommand::DispatchPrompt { owner, turn_id }
            if owner == &context.owner && turn_id == turn.dispatch.turn_id() => {}
        WorkerCommand::BeginClose(command)
            if command.project_id == context.owner.project_id
                && command.chat_id == context.owner.chat_id
                && command.session_id == context.owner.session_id
                && command.runtime_epoch == context.owner.runtime_epoch =>
        {
            drop(prompt);
            return super::idle::begin(
                IdleCloseBeginState::prepared(turn),
                command.clone(),
                envelope.into_parts().1,
            )
            .await;
        }
        _ => {
            envelope.reject(HostError::Intent);
            return FirstTurnWorkerState::Prepared { turn, prompt };
        }
    }
    let running = match turn.bind_driver(context.journal.clone(), Arc::clone(&context.publisher)) {
        Ok(running) => running,
        Err(failure) => {
            envelope.respond(Err(HostError::RecoveryRequired));
            return FirstTurnWorkerState::Recovery(Box::new(FirstTurnWorkerRecovery::Bind(
                failure,
            )));
        }
    };
    write_once(
        PreparedProcessWrite::new(running, prompt),
        context,
        envelope,
    )
    .await
}

pub(super) async fn handle_active<A, R, T, C, Auth>(
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
    match &envelope.command {
        WorkerCommand::ResolvePermission {
            owner,
            request_id,
            decision,
        } if owner == &context.owner => {
            let request_id = request_id.clone();
            let decision = *decision;
            match active
                .turn
                .driver
                .resolve_permission(&request_id, decision)
                .await
            {
                Ok(()) => {
                    envelope.respond(Ok(WorkerReply::Unit));
                    FirstTurnWorkerState::Active(active)
                }
                Err(_) => {
                    envelope.respond(Err(HostError::RecoveryRequired));
                    FirstTurnWorkerState::Active(active)
                }
            }
        }
        WorkerCommand::DispatchPrompt { owner, turn_id }
            if owner == &context.owner
                && turn_id == active.turn.dispatch.turn_id()
                && active.ready_for_write() =>
        {
            write_once(active, context, envelope).await
        }
        WorkerCommand::ObserveSend {
            owner,
            turn_id,
            observation,
        } if owner == &context.owner && turn_id == active.turn.dispatch.turn_id() => {
            let owner = owner.clone();
            let turn_id = turn_id.clone();
            let observation = *observation;
            match active
                .observe(observation, context.clock.now_millis())
                .await
            {
                Ok(active) => {
                    envelope.respond(Ok(WorkerReply::Unit));
                    FirstTurnWorkerState::Active(active)
                }
                Err(failure) if failure.state.projection_retry_pending() => {
                    envelope.respond(Err(map_write(&failure.error)));
                    FirstTurnWorkerState::Recovery(Box::new(FirstTurnWorkerRecovery::Observe {
                        failure,
                        owner,
                        turn_id,
                        observation,
                    }))
                }
                Err(failure) => {
                    classify_observe_failure(failure, owner, turn_id, observation, envelope)
                }
            }
        }
        WorkerCommand::BeginCancel(intent)
            if intent.owner == context.owner
                && intent.turn_id == *active.turn.dispatch.turn_id() =>
        {
            let intent = intent.clone();
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
        WorkerCommand::DispatchCancel(intent)
            if intent.owner == context.owner
                && intent.turn_id == *active.turn.dispatch.turn_id() =>
        {
            match active.dispatch_cancel(intent) {
                Ok(active) => {
                    envelope.respond(Ok(WorkerReply::Unit));
                    FirstTurnWorkerState::Active(active)
                }
                Err(failure) => retain_active_after_write_error(failure, envelope),
            }
        }
        WorkerCommand::AcceptTerminal { owner, cursor }
            if owner == &context.owner && cursor.turn_id() == active.turn.dispatch.turn_id() =>
        {
            let owner = owner.clone();
            let cursor = cursor.clone();
            super::recovery::accept_terminal_once(active, owner, cursor, context, envelope)
        }
        WorkerCommand::BeginClose(command)
            if command.project_id == context.owner.project_id
                && command.chat_id == context.owner.chat_id
                && command.session_id == context.owner.session_id
                && command.runtime_epoch == context.owner.runtime_epoch =>
        {
            super::active_close::begin(
                ActiveCloseBeginState::new(active, command.clone()),
                envelope,
            )
            .await
        }
        _ => {
            envelope.reject(HostError::Intent);
            FirstTurnWorkerState::Active(active)
        }
    }
}

async fn write_once<A, R, T, C, Auth>(
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
    let owner = context.owner.clone();
    let turn_id = active.turn.dispatch.turn_id().clone();
    match active.dispatch_once(context.clock.now_millis()).await {
        Ok((active, write)) => {
            envelope.respond(Ok(WorkerReply::Write(write)));
            FirstTurnWorkerState::Active(active)
        }
        Err(failure) => classify_begin_send_failure(failure, owner, turn_id, envelope),
    }
}

fn map_prepare(error: &PrepareActivationTurnError) -> HostError {
    match error {
        PrepareActivationTurnError::Duplicate | PrepareActivationTurnError::Identity => {
            HostError::Intent
        }
        PrepareActivationTurnError::Prepare(_) => HostError::RecoveryRequired,
    }
}

fn map_prepare_next(error: &PrepareDriverTurnError) -> HostError {
    match error {
        PrepareDriverTurnError::Duplicate | PrepareDriverTurnError::Identity => HostError::Intent,
        PrepareDriverTurnError::MissingLease
        | PrepareDriverTurnError::Prepare(_)
        | PrepareDriverTurnError::Rebind(_) => HostError::RecoveryRequired,
    }
}

pub(super) fn map_write(error: &PreparedWriteError) -> HostError {
    match error {
        PreparedWriteError::Intent | PreparedWriteError::Fence(_) => HostError::Intent,
        PreparedWriteError::Coordinator(_) | PreparedWriteError::Driver(_) => {
            HostError::RecoveryRequired
        }
    }
}
