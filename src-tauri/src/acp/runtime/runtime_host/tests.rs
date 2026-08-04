use std::sync::{Arc, Condvar, Mutex};

use crate::acp::{
    contract::{
        ClientIntentId, ProjectGeneration, ProjectionRevision, RuntimeEpoch, SageAgentSessionId,
        SageChatId, SageProjectId, SageTurnId,
    },
    intent::Prompt,
    lifecycle::{DeliveryState, TurnState},
    scheduler::RuntimeOwner,
    store::AdapterTurnCursor,
};

use super::types::HostCommand;
use super::*;

type PrepareGate = Arc<(Mutex<(bool, bool)>, Condvar)>;

#[derive(Clone)]
struct FakeEngine {
    events: Arc<Mutex<Vec<&'static str>>>,
    writes: Arc<Mutex<Vec<PromptWrite>>>,
    fail_zero: bool,
    prepare_error: Option<HostError>,
    dispatch_error: Option<HostError>,
    retry_error: Option<HostError>,
    observation_error: Option<HostError>,
    terminal_errors: Arc<Mutex<Vec<HostError>>>,
    begin_cancel_errors: Arc<Mutex<Vec<HostError>>>,
    begin_close_errors: Arc<Mutex<Vec<HostError>>>,
    prepare_gate: Option<PrepareGate>,
}

impl FakeEngine {
    fn new(writes: impl IntoIterator<Item = PromptWrite>) -> Self {
        let mut writes = writes.into_iter().collect::<Vec<_>>();
        writes.reverse();
        Self {
            events: Arc::new(Mutex::new(Vec::new())),
            writes: Arc::new(Mutex::new(writes)),
            fail_zero: false,
            prepare_error: None,
            dispatch_error: None,
            retry_error: None,
            observation_error: None,
            terminal_errors: Arc::new(Mutex::new(Vec::new())),
            begin_cancel_errors: Arc::new(Mutex::new(Vec::new())),
            begin_close_errors: Arc::new(Mutex::new(Vec::new())),
            prepare_gate: None,
        }
    }

    fn with_zero_failure(mut self) -> Self {
        self.fail_zero = true;
        self
    }

    fn with_prepare_error(mut self, error: HostError) -> Self {
        self.prepare_error = Some(error);
        self
    }

    fn with_dispatch_error(mut self, error: HostError) -> Self {
        self.dispatch_error = Some(error);
        self
    }

    fn with_retry_error(mut self, error: HostError) -> Self {
        self.retry_error = Some(error);
        self
    }

    fn with_observation_error(mut self, error: HostError) -> Self {
        self.observation_error = Some(error);
        self
    }

    fn with_terminal_errors(self, errors: impl IntoIterator<Item = HostError>) -> Self {
        let mut errors = errors.into_iter().collect::<Vec<_>>();
        errors.reverse();
        *self.terminal_errors.lock().unwrap() = errors;
        self
    }

    fn with_begin_cancel_errors(self, errors: impl IntoIterator<Item = HostError>) -> Self {
        let mut errors = errors.into_iter().collect::<Vec<_>>();
        errors.reverse();
        *self.begin_cancel_errors.lock().unwrap() = errors;
        self
    }

    fn with_begin_close_errors(self, errors: impl IntoIterator<Item = HostError>) -> Self {
        let mut errors = errors.into_iter().collect::<Vec<_>>();
        errors.reverse();
        *self.begin_close_errors.lock().unwrap() = errors;
        self
    }

    fn with_prepare_gate(mut self, gate: Arc<(Mutex<(bool, bool)>, Condvar)>) -> Self {
        self.prepare_gate = Some(gate);
        self
    }

    fn events(&self) -> Vec<&'static str> {
        self.events.lock().unwrap().clone()
    }

    fn push(&self, event: &'static str) {
        self.events.lock().unwrap().push(event);
    }
}

impl RuntimeEngine for FakeEngine {
    fn activate(&mut self, _owner: &RuntimeOwner) -> Result<(), HostError> {
        self.push("activate");
        Ok(())
    }

    fn prepare_prompt(&mut self, _intent: &HostPromptIntent) -> Result<SageTurnId, HostError> {
        self.push("preparePrompt");
        if let Some(gate) = &self.prepare_gate {
            let (state, ready) = &**gate;
            let mut state = state.lock().unwrap();
            state.0 = true;
            ready.notify_all();
            while !state.1 {
                state = ready.wait(state).unwrap();
            }
        }
        if let Some(error) = self.prepare_error.take() {
            return Err(error);
        }
        Ok(turn())
    }

    fn dispatch_prompt(
        &mut self,
        _owner: &RuntimeOwner,
        _turn_id: &SageTurnId,
    ) -> Result<PromptWrite, HostError> {
        self.push("dispatchPrompt");
        if let Some(error) = self.dispatch_error.take() {
            return Err(error);
        }
        self.next_write()
    }

    fn retry_prompt(
        &mut self,
        _owner: &RuntimeOwner,
        _turn_id: &SageTurnId,
    ) -> Result<PromptWrite, HostError> {
        self.push("retryPrompt");
        if let Some(error) = self.retry_error.take() {
            return Err(error);
        }
        self.next_write()
    }

    fn record_delivery_unknown(
        &mut self,
        _owner: &RuntimeOwner,
        _turn_id: &SageTurnId,
    ) -> Result<(), HostError> {
        self.push("deliveryUnknown");
        if let Some(error) = self.observation_error.take() {
            return Err(error);
        }
        Ok(())
    }

    fn record_retry_exhausted(
        &mut self,
        _owner: &RuntimeOwner,
        _turn_id: &SageTurnId,
    ) -> Result<(), HostError> {
        self.push("retryExhausted");
        Ok(())
    }

    fn begin_cancel(&mut self, _intent: &HostCancelIntent) -> Result<(), HostError> {
        self.push("beginCancel");
        if let Some(error) = self.begin_cancel_errors.lock().unwrap().pop() {
            return Err(error);
        }
        Ok(())
    }

    fn dispatch_cancel(&mut self, _intent: &HostCancelIntent) -> Result<(), HostError> {
        self.push("dispatchCancel");
        Ok(())
    }

    fn accept_terminal(&mut self, _cursor: &AdapterTurnCursor) -> Result<(), HostError> {
        self.push("acceptTerminal");
        if let Some(error) = self.terminal_errors.lock().unwrap().pop() {
            return Err(error);
        }
        Ok(())
    }

    fn begin_close(&mut self, _intent: &HostCloseIntent) -> Result<(), HostError> {
        self.push("beginClose");
        if let Some(error) = self.begin_close_errors.lock().unwrap().pop() {
            return Err(error);
        }
        Ok(())
    }

    fn converge_zero(&mut self, _intent: &HostCloseIntent) -> Result<(), HostError> {
        self.push("convergeZero");
        if self.fail_zero {
            Err(HostError::RecoveryRequired)
        } else {
            Ok(())
        }
    }

    fn finalize_close(&mut self, _intent: &HostCloseIntent) -> Result<(), HostError> {
        self.push("finalizeClose");
        Ok(())
    }

    fn shutdown(&mut self) -> Result<(), HostError> {
        self.push("shutdown");
        Ok(())
    }
}

impl FakeEngine {
    fn next_write(&self) -> Result<PromptWrite, HostError> {
        Ok(self
            .writes
            .lock()
            .unwrap()
            .pop()
            .unwrap_or_else(|| PromptWrite::Complete(turn())))
    }
}

fn owner(project: &str) -> RuntimeOwner {
    RuntimeOwner {
        project_id: SageProjectId::parse(project).unwrap(),
        chat_id: SageChatId::parse("chat-host").unwrap(),
        session_id: SageAgentSessionId::parse("session-host").unwrap(),
        runtime_epoch: RuntimeEpoch::new(1).unwrap(),
    }
}

fn prompt(owner: &RuntimeOwner) -> HostPromptIntent {
    HostPromptIntent {
        command: crate::acp::store::PrepareTurnCommand {
            project_id: owner.project_id.clone(),
            project_generation: ProjectGeneration::new(1).unwrap(),
            chat_id: owner.chat_id.clone(),
            session_id: owner.session_id.clone(),
            runtime_epoch: owner.runtime_epoch,
            expected_projection_revision: ProjectionRevision::ZERO,
            client_intent_id: ClientIntentId::parse("intent-prompt").unwrap(),
            prompt: Prompt::new("prompt", "hello").unwrap(),
            now_millis: crate::persistence::now_millis(),
        },
    }
}

fn turn() -> SageTurnId {
    SageTurnId::parse("turn-host").unwrap()
}

fn other_turn() -> SageTurnId {
    SageTurnId::parse("turn-other").unwrap()
}

fn cancel(owner: &RuntimeOwner) -> HostCancelIntent {
    HostCancelIntent {
        owner: owner.clone(),
        turn_id: SageTurnId::parse("turn-host").unwrap(),
        client_intent_id: ClientIntentId::parse("intent-cancel").unwrap(),
        now_millis: crate::persistence::now_millis(),
    }
}

fn close(owner: &RuntimeOwner) -> HostCloseIntent {
    HostCloseIntent {
        command: crate::acp::store::CloseSessionCommand {
            project_id: owner.project_id.clone(),
            chat_id: owner.chat_id.clone(),
            expected_projection_revision: ProjectionRevision::ZERO,
            client_intent_id: ClientIntentId::parse("intent-close").unwrap(),
            session_id: owner.session_id.clone(),
            runtime_epoch: owner.runtime_epoch,
            now_millis: crate::persistence::now_millis(),
        },
    }
}

#[test]
fn active_turn_survives_project_switch_and_cancel_commits_before_dispatch() {
    let engine = FakeEngine::new([PromptWrite::Complete(turn())]);
    let observed = engine.clone();
    let host = RuntimeHost::start(engine).unwrap();
    let runtime = owner("project-a");

    assert_eq!(
        host.activate(runtime.clone()).unwrap(),
        HostOutcome::Activated
    );
    assert_eq!(
        host.send(prompt(&runtime)).unwrap(),
        HostOutcome::PromptAccepted
    );
    assert_eq!(
        host.project_switched(SageProjectId::parse("project-b").unwrap())
            .unwrap(),
        HostOutcome::BackgroundTurnContinues
    );
    assert_eq!(host.state().unwrap(), HostState::Running);
    assert_eq!(
        host.cancel(cancel(&runtime)).unwrap(),
        HostOutcome::CancelQueued
    );
    assert_eq!(
        observed.events(),
        [
            "activate",
            "preparePrompt",
            "dispatchPrompt",
            "beginCancel",
            "dispatchCancel"
        ]
    );
    host.shutdown().unwrap();
}

#[test]
fn post_commit_cancel_begin_failure_retries_only_the_exact_begin_intent() {
    let engine = FakeEngine::new([PromptWrite::Complete(turn())])
        .with_begin_cancel_errors([HostError::RecoveryRequired]);
    let observed = engine.clone();
    let host = RuntimeHost::start(engine).unwrap();
    let runtime = owner("project-a");
    host.activate(runtime.clone()).unwrap();
    host.send(prompt(&runtime)).unwrap();
    let intent = cancel(&runtime);

    assert_eq!(
        host.cancel(intent.clone()),
        Err(HostError::RecoveryRequired)
    );
    assert_eq!(host.state().unwrap(), HostState::Recovery);
    let mut foreign = intent.clone();
    foreign.client_intent_id = ClientIntentId::parse("intent-cancel-foreign").unwrap();
    assert_eq!(host.cancel(foreign), Err(HostError::Intent));
    assert_eq!(host.cancel(intent).unwrap(), HostOutcome::CancelQueued);
    assert_eq!(host.state().unwrap(), HostState::Running);
    assert_eq!(
        observed.events(),
        [
            "activate",
            "preparePrompt",
            "dispatchPrompt",
            "beginCancel",
            "beginCancel",
            "dispatchCancel"
        ]
    );
    host.shutdown().unwrap();
}

#[test]
fn project_switch_between_durable_prepare_and_process_dispatch_keeps_exact_turn_intent() {
    let engine = FakeEngine::new([PromptWrite::Complete(turn())]);
    let observed = engine.clone();
    let host = RuntimeHost::start(engine).unwrap();
    let runtime = owner("project-a");
    host.activate(runtime.clone()).unwrap();

    let prepared = host.prepare_prompt(prompt(&runtime)).unwrap();
    assert_eq!(prepared, turn());
    assert_eq!(
        host.project_switched(SageProjectId::parse("project-b").unwrap())
            .unwrap(),
        HostOutcome::BackgroundTurnContinues
    );
    assert_eq!(host.cancel(cancel(&runtime)), Err(HostError::Intent));
    assert_eq!(
        host.dispatch_prepared(runtime, prepared).unwrap(),
        HostOutcome::PromptAccepted
    );
    assert_eq!(
        observed.events(),
        ["activate", "preparePrompt", "dispatchPrompt"]
    );
    host.shutdown().unwrap();
}

#[test]
fn possible_bytes_are_terminally_recorded_without_prompt_replay() {
    let engine = FakeEngine::new([
        PromptWrite::PossibleBytes(turn()),
        PromptWrite::Complete(turn()),
    ]);
    let observed = engine.clone();
    let host = RuntimeHost::start(engine).unwrap();
    let runtime = owner("project-a");
    host.activate(runtime.clone()).unwrap();

    assert_eq!(host.send(prompt(&runtime)), Err(HostError::DeliveryUnknown));
    assert_eq!(host.state().unwrap(), HostState::Recovery);
    assert_eq!(
        observed.events(),
        [
            "activate",
            "preparePrompt",
            "dispatchPrompt",
            "deliveryUnknown"
        ]
    );
    host.shutdown().unwrap();
}

#[test]
fn failed_first_dispatch_enters_recovery_and_cannot_replay_the_prepared_turn() {
    let engine = FakeEngine::new([]).with_dispatch_error(HostError::Unavailable);
    let observed = engine.clone();
    let host = RuntimeHost::start(engine).unwrap();
    let runtime = owner("project-a");
    host.activate(runtime.clone()).unwrap();
    let prepared = host.prepare_prompt(prompt(&runtime)).unwrap();

    assert_eq!(
        host.dispatch_prepared(runtime.clone(), prepared.clone()),
        Err(HostError::Unavailable)
    );
    assert_eq!(host.state().unwrap(), HostState::Recovery);
    assert_eq!(
        host.dispatch_prepared(runtime, prepared),
        Err(HostError::Intent)
    );
    assert_eq!(
        observed.events(),
        ["activate", "preparePrompt", "dispatchPrompt"]
    );
    host.shutdown().unwrap();
}

#[test]
fn failed_zero_byte_retry_enters_recovery_and_cannot_replay() {
    let engine =
        FakeEngine::new([PromptWrite::ZeroBytes(turn())]).with_retry_error(HostError::Unavailable);
    let observed = engine.clone();
    let host = RuntimeHost::start(engine).unwrap();
    let runtime = owner("project-a");
    host.activate(runtime.clone()).unwrap();

    assert_eq!(host.send(prompt(&runtime)), Err(HostError::Unavailable));
    assert_eq!(host.state().unwrap(), HostState::Recovery);
    assert_eq!(
        observed.events(),
        ["activate", "preparePrompt", "dispatchPrompt", "retryPrompt"]
    );
    host.shutdown().unwrap();
}

#[test]
fn failed_delivery_unknown_record_enters_recovery_without_replay() {
    let engine = FakeEngine::new([PromptWrite::PossibleBytes(turn())])
        .with_observation_error(HostError::RecoveryRequired);
    let observed = engine.clone();
    let host = RuntimeHost::start(engine).unwrap();
    let runtime = owner("project-a");
    host.activate(runtime.clone()).unwrap();

    assert_eq!(
        host.send(prompt(&runtime)),
        Err(HostError::RecoveryRequired)
    );
    assert_eq!(host.state().unwrap(), HostState::Recovery);
    assert_eq!(
        observed.events(),
        [
            "activate",
            "preparePrompt",
            "dispatchPrompt",
            "deliveryUnknown"
        ]
    );
    host.shutdown().unwrap();
}

#[test]
fn uncertain_prepare_failure_enters_recovery_and_cannot_prepare_again() {
    let engine = FakeEngine::new([]).with_prepare_error(HostError::RecoveryRequired);
    let observed = engine.clone();
    let host = RuntimeHost::start(engine).unwrap();
    let runtime = owner("project-a");
    host.activate(runtime.clone()).unwrap();

    assert_eq!(
        host.prepare_prompt(prompt(&runtime)),
        Err(HostError::RecoveryRequired)
    );
    assert_eq!(host.state().unwrap(), HostState::Recovery);
    assert_eq!(
        host.prepare_prompt(prompt(&runtime)),
        Err(HostError::Intent)
    );
    assert_eq!(observed.events(), ["activate", "preparePrompt"]);
    host.shutdown().unwrap();
}

#[test]
fn zero_byte_retry_cannot_substitute_a_different_store_minted_turn() {
    let engine = FakeEngine::new([
        PromptWrite::ZeroBytes(turn()),
        PromptWrite::Complete(other_turn()),
    ]);
    let host = RuntimeHost::start(engine).unwrap();
    let runtime = owner("project-a");
    host.activate(runtime.clone()).unwrap();

    assert_eq!(host.send(prompt(&runtime)), Err(HostError::Intent));
    assert_eq!(host.state().unwrap(), HostState::Recovery);
    host.shutdown().unwrap();
}

#[test]
fn zero_bytes_retry_once_and_second_failure_is_durable() {
    let engine = FakeEngine::new([
        PromptWrite::ZeroBytes(turn()),
        PromptWrite::ZeroBytes(turn()),
    ]);
    let observed = engine.clone();
    let host = RuntimeHost::start(engine).unwrap();
    let runtime = owner("project-a");
    host.activate(runtime.clone()).unwrap();

    assert_eq!(host.send(prompt(&runtime)), Err(HostError::RetryExhausted));
    assert_eq!(host.state().unwrap(), HostState::Recovery);
    assert_eq!(
        observed.events(),
        [
            "activate",
            "preparePrompt",
            "dispatchPrompt",
            "retryPrompt",
            "retryExhausted"
        ]
    );
    host.shutdown().unwrap();
}

#[test]
fn terminal_cursor_returns_idle_and_close_orders_zero_before_finalize() {
    let engine = FakeEngine::new([PromptWrite::Complete(turn())]);
    let observed = engine.clone();
    let host = RuntimeHost::start(engine).unwrap();
    let runtime = owner("project-a");
    let turn_id = SageTurnId::parse("turn-host").unwrap();
    host.activate(runtime.clone()).unwrap();
    host.send(prompt(&runtime)).unwrap();
    let cursor = AdapterTurnCursor::test_only(
        turn_id,
        TurnState::Completed,
        DeliveryState::Correlated,
        crate::acp::contract::EntityRevision::new(2).unwrap(),
        crate::acp::contract::ProjectionRevision::new(3).unwrap(),
    );

    assert_eq!(
        host.accept_terminal(runtime.clone(), cursor).unwrap(),
        HostOutcome::TurnTerminal
    );
    assert_eq!(host.state().unwrap(), HostState::Idle);
    assert_eq!(host.close(close(&runtime)).unwrap(), HostOutcome::Closed);
    assert_eq!(
        observed.events(),
        [
            "activate",
            "preparePrompt",
            "dispatchPrompt",
            "acceptTerminal",
            "beginClose",
            "convergeZero",
            "finalizeClose",
        ]
    );
    host.shutdown().unwrap();
}

#[test]
fn terminal_acceptance_failure_retries_only_the_exact_cursor_and_releases_once() {
    let engine = FakeEngine::new([PromptWrite::Complete(turn())])
        .with_terminal_errors([HostError::RecoveryRequired]);
    let observed = engine.clone();
    let host = RuntimeHost::start(engine).unwrap();
    let runtime = owner("project-a");
    host.activate(runtime.clone()).unwrap();
    host.send(prompt(&runtime)).unwrap();
    let cursor = AdapterTurnCursor::test_only(
        turn(),
        TurnState::Completed,
        DeliveryState::Correlated,
        crate::acp::contract::EntityRevision::new(2).unwrap(),
        crate::acp::contract::ProjectionRevision::new(3).unwrap(),
    );

    assert_eq!(
        host.accept_terminal(runtime.clone(), cursor.clone()),
        Err(HostError::RecoveryRequired)
    );
    assert_eq!(host.state().unwrap(), HostState::Recovery);
    assert_eq!(
        host.accept_terminal(owner("project-foreign"), cursor.clone()),
        Err(HostError::Intent)
    );
    assert_eq!(
        host.accept_terminal(runtime, cursor).unwrap(),
        HostOutcome::TurnTerminal
    );
    assert_eq!(host.state().unwrap(), HostState::Idle);
    assert_eq!(
        observed.events(),
        [
            "activate",
            "preparePrompt",
            "dispatchPrompt",
            "acceptTerminal",
            "acceptTerminal"
        ]
    );
    host.shutdown().unwrap();
}

#[test]
fn close_convergence_failure_retains_recovery_and_never_finalizes() {
    let engine = FakeEngine::new([]).with_zero_failure();
    let observed = engine.clone();
    let host = RuntimeHost::start(engine).unwrap();
    let runtime = owner("project-a");
    host.activate(runtime.clone()).unwrap();

    assert_eq!(
        host.close(close(&runtime)),
        Err(HostError::RecoveryRequired)
    );
    assert_eq!(host.state().unwrap(), HostState::Recovery);
    assert_eq!(
        observed.events(),
        ["activate", "beginClose", "convergeZero"]
    );
    host.shutdown().unwrap();
}

#[test]
fn post_commit_begin_close_failure_retains_exact_intent_and_retries_the_begin_phase() {
    let engine = FakeEngine::new([]).with_begin_close_errors([HostError::RecoveryRequired]);
    let observed = engine.clone();
    let host = RuntimeHost::start(engine).unwrap();
    let runtime = owner("project-a");
    let intent = close(&runtime);
    host.activate(runtime.clone()).unwrap();

    assert_eq!(
        host.begin_close(intent.clone()),
        Err(HostError::RecoveryRequired)
    );
    assert_eq!(host.state().unwrap(), HostState::Recovery);
    assert_eq!(
        host.recover_close(close(&owner("project-foreign"))),
        Err(HostError::Intent)
    );
    assert_eq!(host.recover_close(intent).unwrap(), HostOutcome::Closed);
    assert_eq!(
        observed.events(),
        [
            "activate",
            "beginClose",
            "beginClose",
            "convergeZero",
            "finalizeClose"
        ]
    );
    host.shutdown().unwrap();
}

#[test]
fn close_releases_the_caller_between_durable_begin_and_zero_process_wait() {
    let engine = FakeEngine::new([]);
    let observed = engine.clone();
    let host = RuntimeHost::start(engine).unwrap();
    let runtime = owner("project-a");
    let intent = close(&runtime);
    host.activate(runtime).unwrap();

    assert_eq!(
        host.begin_close(intent.clone()).unwrap(),
        HostOutcome::CloseStarted
    );
    assert_eq!(observed.events(), ["activate", "beginClose"]);
    assert_eq!(host.finish_close(intent).unwrap(), HostOutcome::Closed);
    assert_eq!(
        observed.events(),
        ["activate", "beginClose", "convergeZero", "finalizeClose"]
    );
    host.shutdown().unwrap();
}

#[test]
fn shutdown_is_a_barrier_after_the_in_flight_call_and_rejects_queued_data() {
    use std::sync::mpsc;

    use super::mailbox::Envelope;
    use super::types::{HostCommand, HostReply};

    let gate = Arc::new((Mutex::new((false, false)), Condvar::new()));
    let engine = FakeEngine::new([]).with_prepare_gate(Arc::clone(&gate));
    let observed = engine.clone();
    let host = RuntimeHost::start(engine).unwrap();
    let runtime = owner("project-a");
    host.activate(runtime.clone()).unwrap();

    let (prepare_reply, prepare_response) = mpsc::sync_channel(1);
    host.mailbox
        .try_push(Envelope {
            command: HostCommand::PreparePrompt(prompt(&runtime)),
            reply: prepare_reply,
        })
        .unwrap();
    let (state, ready) = &*gate;
    let mut blocked = state.lock().unwrap();
    while !blocked.0 {
        blocked = ready.wait(blocked).unwrap();
    }

    let (data_reply, data_response) = mpsc::sync_channel(1);
    host.mailbox
        .try_push(Envelope {
            command: HostCommand::ProjectSwitched(SageProjectId::parse("project-b").unwrap()),
            reply: data_reply,
        })
        .unwrap();
    let (shutdown_reply, shutdown_response) = mpsc::sync_channel(1);
    host.mailbox
        .try_push(Envelope {
            command: HostCommand::Shutdown,
            reply: shutdown_reply,
        })
        .unwrap();
    blocked.1 = true;
    ready.notify_all();
    drop(blocked);

    assert!(matches!(
        prepare_response.recv().unwrap(),
        Ok(HostReply::Prepared(_))
    ));
    assert!(matches!(
        shutdown_response.recv().unwrap(),
        Ok(HostReply::Outcome(HostOutcome::Shutdown))
    ));
    assert!(matches!(
        data_response.recv().unwrap(),
        Err(HostError::Closed)
    ));
    assert_eq!(observed.events(), ["activate", "preparePrompt", "shutdown"]);
    host.join().unwrap();
}

#[test]
fn bounded_data_saturation_cannot_consume_reserved_control_capacity() {
    use std::sync::mpsc;

    use super::mailbox::{Envelope, HostMailbox};
    use super::types::HostCommand;

    let mailbox = HostMailbox::new().unwrap();
    for index in 0..8 {
        let (reply, _response) = mpsc::sync_channel(1);
        mailbox
            .try_push(Envelope {
                command: HostCommand::ProjectSwitched(
                    SageProjectId::parse(format!("project-{index}")).unwrap(),
                ),
                reply,
            })
            .unwrap();
    }
    let (reply, _response) = mpsc::sync_channel(1);
    assert!(matches!(
        mailbox.try_push(Envelope {
            command: HostCommand::ProjectSwitched(SageProjectId::parse("project-full").unwrap()),
            reply,
        }),
        Err(HostError::Capacity)
    ));

    let (reply, _response) = mpsc::sync_channel(1);
    mailbox
        .try_push(Envelope {
            command: HostCommand::State,
            reply,
        })
        .unwrap();
    mailbox.close();
}

#[test]
fn closing_the_host_mailbox_replies_closed_to_every_queued_caller() {
    use std::{sync::mpsc, time::Duration};

    use super::mailbox::{Envelope, HostMailbox};
    use super::types::HostCommand;

    let mailbox = HostMailbox::new().unwrap();
    let mut responses = Vec::new();
    for command in [
        HostCommand::ProjectSwitched(SageProjectId::parse("project-queued").unwrap()),
        HostCommand::State,
        HostCommand::Shutdown,
    ] {
        let (reply, response) = mpsc::sync_channel(1);
        mailbox.try_push(Envelope { command, reply }).unwrap();
        responses.push(response);
    }

    mailbox.close();
    for response in responses {
        assert!(matches!(
            response.recv_timeout(Duration::from_millis(100)).unwrap(),
            Err(HostError::Closed)
        ));
    }
}

#[test]
fn nonresponding_actor_returns_at_deadline_and_closes_queued_work() {
    use std::time::{Duration, Instant};

    let gate = Arc::new((Mutex::new((false, false)), Condvar::new()));
    let engine = FakeEngine::new([]).with_prepare_gate(Arc::clone(&gate));
    let host = RuntimeHost::start_with_timeout(engine, Duration::from_millis(20)).unwrap();
    let runtime = owner("project-a");
    host.activate(runtime.clone()).unwrap();

    let started = Instant::now();
    assert_eq!(
        host.prepare_prompt(prompt(&runtime)),
        Err(HostError::Deadline)
    );
    assert!(started.elapsed() < Duration::from_secs(1));
    assert_eq!(host.state(), Err(HostError::RecoveryRequired));

    let (state, ready) = &*gate;
    let mut state = state.lock().unwrap();
    state.1 = true;
    ready.notify_all();
    drop(state);
    assert_eq!(host.shutdown(), Ok(()));
}

#[test]
fn command_deadlines_use_the_frozen_operation_classes() {
    use crate::acp::limits::LIMITS;

    let runtime = owner("project-a");
    assert_eq!(
        HostCommand::Activate(runtime.clone()).deadline_millis(),
        LIMITS.deadlines_ms.initialize
    );
    assert_eq!(
        HostCommand::Cancel(cancel(&runtime)).deadline_millis(),
        LIMITS.deadlines_ms.tree_termination
    );
    assert_eq!(
        HostCommand::PreparePrompt(prompt(&runtime)).deadline_millis(),
        LIMITS.deadlines_ms.ordinary_rpc
    );
    assert_eq!(
        HostCommand::Shutdown.deadline_millis(),
        LIMITS.deadlines_ms.explicit_close
    );
}
