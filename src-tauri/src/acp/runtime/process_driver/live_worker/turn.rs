use std::sync::Arc;

use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    runtime::{
        CoordinatorActivationJournal, HostError, ProjectIntentAuthority, SqliteAdapterBatchJournal,
    },
};

use super::super::{
    ActiveCloseBeginState, CompletedActivationDriverParts, CompletedDriverParts,
    CoordinatorZeroToken, IdleCloseBeginState, PreparedActivationTurn, PreparedProcessWrite,
    ProcessTreeControl, ReusableCloseBeginState, StdinTransport, WorkerCommand, WorkerEnvelope,
    WorkerReply,
};
use super::write_recovery::{
    classify_begin_send_failure, classify_observe_failure, durable_commit_unknown, map_write,
    retain_active_after_write_error,
};
use super::{FirstTurnWorkerContext, FirstTurnWorkerRecovery, FirstTurnWorkerState};

#[path = "turn/prepare_error.rs"]
mod prepare_error;
use prepare_error::{map_prepare, map_prepare_next};

pub(super) async fn handle_reusable<A, R, T, C, Auth>(
    parts: Box<
        CompletedDriverParts<
            A,
            SqliteAdapterBatchJournal,
            R,
            T,
            C,
            crate::acp::runtime::SessionTreeLease,
        >,
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
            if let Err(error) = parts
                .completed
                .validate_command_prompt(&intent.command.prompt)
            {
                responder.respond(Err(error));
                return FirstTurnWorkerState::Reusable(parts);
            }
            if !parts.completed.idle_frame_aligned() {
                responder.respond(Err(HostError::Capacity));
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
                    FirstTurnWorkerState::Active(Box::new(PreparedProcessWrite::new(
                        turn,
                        prompt,
                        intent.access_mode,
                    )))
                }
                Err(failure) => {
                    responder.respond(Err(map_prepare_next(&failure.error)));
                    FirstTurnWorkerState::Recovery(Box::new(FirstTurnWorkerRecovery::PrepareNext(
                        failure,
                    )))
                }
            }
        }
        WorkerCommand::AcceptLatestCompleted => {
            responder.respond(Ok(WorkerReply::Unit));
            FirstTurnWorkerState::Reusable(parts)
        }
        WorkerCommand::BeginClose(command) => {
            super::reusable::begin(ReusableCloseBeginState::new(*parts, command), responder).await
        }
        WorkerCommand::BeginPark => match ReusableCloseBeginState::begin_park(*parts).await {
            Ok(closing) => {
                responder.respond(Ok(WorkerReply::Unit));
                FirstTurnWorkerState::ReusableDraining(Box::new(closing))
            }
            Err(failure) => {
                responder.respond(Err(super::map_close(&failure.error)));
                FirstTurnWorkerState::Recovery(Box::new(FirstTurnWorkerRecovery::ReusableBegin(
                    failure,
                )))
            }
        },
        _ => {
            responder.respond(Err(HostError::State));
            FirstTurnWorkerState::Reusable(parts)
        }
    }
}
pub(super) async fn handle_idle<A, R, T, C, Auth>(
    parts: Box<CompletedActivationDriverParts<A, CoordinatorActivationJournal, R, T, C>>,
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
            if let Err(error) = parts.parts.validate_command_prompt(&intent.command.prompt) {
                responder.respond(Err(error));
                return FirstTurnWorkerState::Idle(parts);
            }
            if !parts.idle_decoder.is_frame_aligned() {
                responder.respond(Err(HostError::Capacity));
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
                    FirstTurnWorkerState::Prepared {
                        turn: Box::new(turn),
                        prompt,
                        access_mode: intent.access_mode,
                    }
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
            super::idle::begin(IdleCloseBeginState::activation(*parts), command, responder).await
        }
        _ => {
            responder.respond(Err(HostError::State));
            FirstTurnWorkerState::Idle(parts)
        }
    }
}

pub(super) async fn handle_prepared<A, R, T, C, Auth>(
    turn: Box<PreparedActivationTurn<A, R, T, C>>,
    prompt: crate::acp::intent::Prompt,
    access_mode: crate::acp_protocol::access_mode::SessionAccessMode,
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
                IdleCloseBeginState::prepared(*turn),
                command.clone(),
                envelope.into_parts().1,
            )
            .await;
        }
        _ => {
            envelope.reject(HostError::Intent);
            return FirstTurnWorkerState::Prepared {
                turn,
                prompt,
                access_mode,
            };
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
        PreparedProcessWrite::new(running, prompt, access_mode),
        context,
        envelope,
    )
    .await
}

pub(super) async fn handle_active<A, R, T, C, Auth>(
    mut active: Box<PreparedProcessWrite<A, SqliteAdapterBatchJournal, R, T, C>>,
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
            let decision = decision.clone();
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
            write_once(*active, context, envelope).await
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
                    FirstTurnWorkerState::Active(Box::new(active))
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
            active.reject_steer(HostError::Intent);
            let intent = intent.clone();
            match active.begin_cancel(&intent).await {
                Ok(active) => {
                    envelope.respond(Ok(WorkerReply::Unit));
                    FirstTurnWorkerState::Active(Box::new(active))
                }
                Err(failure) if durable_commit_unknown(&failure.error) => {
                    envelope.respond(Err(map_write(&failure.error)));
                    FirstTurnWorkerState::Active(Box::new(
                        failure.state.mark_durable_mutation_unknown(),
                    ))
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
            match active.dispatch_cancel(intent).await {
                Ok(active) => {
                    envelope.respond(Ok(WorkerReply::Unit));
                    FirstTurnWorkerState::Active(Box::new(active))
                }
                Err(failure) => retain_active_after_write_error(failure, envelope),
            }
        }
        WorkerCommand::Steer { .. } => {
            let deadline = envelope.deadline();
            let (command, responder) = envelope.into_parts();
            let WorkerCommand::Steer { intent, completion } = command else {
                unreachable!("Steer command matched")
            };
            super::turn_steer::dispatch(&mut active, intent, deadline, completion, responder);
            FirstTurnWorkerState::Active(active)
        }
        WorkerCommand::AcceptTerminal { owner, cursor }
            if owner == &context.owner && cursor.turn_id() == active.turn.dispatch.turn_id() =>
        {
            let owner = owner.clone();
            let cursor = cursor.clone();
            super::recovery::accept_terminal_once(*active, owner, cursor, context, envelope)
        }
        WorkerCommand::AcceptLatestCompleted => {
            super::turn_park::accept_latest_completed(*active, context, envelope)
        }
        WorkerCommand::WaitZero { .. } => {
            super::turn_park::wait_zero_active(*active, context, envelope).await
        }
        WorkerCommand::ParkAfterZero { .. } => {
            super::turn_park::park_after_zero_active(*active, context, envelope)
        }
        WorkerCommand::BeginClose(command)
            if command.project_id == context.owner.project_id
                && command.chat_id == context.owner.chat_id
                && command.session_id == context.owner.session_id
                && command.runtime_epoch == context.owner.runtime_epoch =>
        {
            active.reject_steer(HostError::Closed);
            super::active_close::begin(
                ActiveCloseBeginState::new(*active, command.clone()),
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
            FirstTurnWorkerState::Active(Box::new(active))
        }
        Err(failure) => classify_begin_send_failure(failure, owner, turn_id, envelope),
    }
}
