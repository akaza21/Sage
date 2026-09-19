use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    runtime::{ConfigCompletionSink, DriverClock, DriverError, HostError, ProjectIntentAuthority},
    store::AdapterTurnCursor,
};

use super::super::WorkerCommand;
use super::{
    DriverStep, FirstTurnWorkerContext, FirstTurnWorkerState, RunnerAction, StdinTransport,
    WorkerEnvelope, WorkerState, terminal_cursor,
};

pub(super) struct PendingConfig {
    completion: ConfigCompletionSink,
    deadline: std::time::Instant,
}

impl PendingConfig {
    pub(super) fn complete(self, result: Result<(), HostError>) {
        self.completion.complete(result);
    }

    pub(super) fn deadline_millis(&self, clock: &dyn DriverClock) -> i64 {
        let remaining = self
            .deadline
            .saturating_duration_since(std::time::Instant::now());
        let remaining_millis = i64::try_from(remaining.as_millis()).unwrap_or(i64::MAX);
        clock.now_millis().saturating_add(remaining_millis)
    }

    pub(super) fn expired(&self) -> bool {
        std::time::Instant::now() >= self.deadline
    }
}

pub(super) async fn handle_worker<A, R, T, Auth>(
    state: WorkerState<A, R, T>,
    context: &FirstTurnWorkerContext<Auth>,
    envelope: WorkerEnvelope,
    pending_config: &mut Option<PendingConfig>,
) -> (RunnerAction<A, R, T>, Option<AdapterTurnCursor>)
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
    Auth: ProjectIntentAuthority,
{
    if matches!(&envelope.command, WorkerCommand::Shutdown) {
        if let Some(config) = pending_config.take() {
            config.complete(Err(HostError::Closed));
        }
        let mut state = state;
        state.close_stdin_after_durable_completed();
        return (RunnerAction::Shutdown(Box::new(state), envelope), None);
    }
    if let WorkerCommand::AbandonConfiguration { owner } = &envelope.command {
        if owner != &context.owner {
            envelope.reject(HostError::Intent);
            return (RunnerAction::Continue(state), None);
        }
        let Some(config) = pending_config.take() else {
            envelope.reject(HostError::State);
            return (RunnerAction::Continue(state), None);
        };
        config.complete(Err(HostError::Closed));
        envelope.respond(Ok(super::super::WorkerReply::Unit));
        return (RunnerAction::Continue(state), None);
    }
    if pending_config.is_some()
        && matches!(
            &envelope.command,
            WorkerCommand::BeginClose(_) | WorkerCommand::BeginPark
        )
    {
        pending_config
            .take()
            .expect("checked pending config")
            .complete(Err(HostError::Closed));
    }
    if matches!(&envelope.command, WorkerCommand::ConfigureSession { .. }) {
        let mut state = state;
        let deadline = envelope.deadline();
        let (command, responder) = envelope.into_parts();
        let WorkerCommand::ConfigureSession { intent, completion } = command else {
            unreachable!("configuration command matched")
        };
        if pending_config.is_some() {
            responder.respond(Err(HostError::State));
            completion.complete(Err(HostError::State));
            return (RunnerAction::Continue(state), None);
        }
        match state.begin_configure(context, &intent, deadline) {
            Ok(()) => {
                responder.respond(Ok(super::super::WorkerReply::Unit));
                *pending_config = Some(PendingConfig {
                    completion,
                    deadline,
                });
            }
            Err(error) => {
                responder.respond(Err(error.clone()));
                completion.complete(Err(error));
            }
        }
        return (RunnerAction::Continue(state), None);
    }
    let mut terminal_callback = terminal_cursor(&envelope);
    let mut state = state.handle(context, envelope).await;
    loop {
        let FirstTurnWorkerState::Active(mut active) = state else {
            return (RunnerAction::Continue(state), terminal_callback);
        };
        if !active.stdout_allowed() {
            return (
                RunnerAction::Continue(FirstTurnWorkerState::Active(active)),
                terminal_callback,
            );
        }
        match active
            .turn
            .driver
            .resume_after_worker_control(context.mailbox.as_ref())
            .await
        {
            Ok(DriverStep::Progress(_)) => {
                return (
                    RunnerAction::Continue(FirstTurnWorkerState::Active(active)),
                    terminal_callback,
                );
            }
            Ok(DriverStep::YieldWorker { envelope, .. }) => {
                if matches!(&envelope.command, WorkerCommand::Shutdown) {
                    let mut state = FirstTurnWorkerState::Active(active);
                    state.close_stdin_after_durable_completed();
                    return (
                        RunnerAction::Shutdown(Box::new(state), envelope),
                        terminal_callback,
                    );
                }
                if let Some(cursor) = terminal_cursor(&envelope)
                    && terminal_callback.replace(cursor).is_some()
                {
                    return (
                        RunnerAction::Failure(FirstTurnWorkerState::Active(active)),
                        terminal_callback,
                    );
                }
                state = FirstTurnWorkerState::Active(active)
                    .handle(context, envelope)
                    .await;
            }
            Err(DriverError::ConfigMutationRejected) => {
                let Some(config) = pending_config.take() else {
                    return (
                        RunnerAction::Failure(FirstTurnWorkerState::Active(active)),
                        terminal_callback,
                    );
                };
                config.complete(Err(HostError::ConfigMutationRejected));
                return (
                    RunnerAction::Continue(FirstTurnWorkerState::Active(active)),
                    terminal_callback,
                );
            }
            Err(_) => {
                return (
                    RunnerAction::Failure(FirstTurnWorkerState::Active(active)),
                    terminal_callback,
                );
            }
        }
    }
}
