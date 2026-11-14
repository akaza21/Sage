use std::{
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant},
};

use crate::acp::{
    contract::{
        ClientIntentId, ProjectGeneration, ProjectionRevision, RuntimeEpoch, SageAgentSessionId,
        SageChatId, SageProjectId, SageTurnId,
    },
    intent::Prompt,
    lifecycle::{DeliveryState, TurnState},
    runtime::LiveRuntimePort,
    scheduler::RuntimeOwner,
    store::AdapterTurnCursor,
};

use super::types::HostCommand;
use super::*;

type PrepareGate = Arc<(Mutex<(bool, bool)>, Condvar)>;
type TerminalGate = Arc<(Mutex<(bool, bool)>, Condvar)>;
type ConfigCompletion = Arc<Mutex<Option<ConfigCompletionSink>>>;
type NativeCompletion = Arc<Mutex<Option<NativeRpcCompletionSink>>>;

fn test_deadline() -> Instant {
    Instant::now() + Duration::from_secs(1)
}

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
    terminal_gate: Option<TerminalGate>,
    config_completion: Option<ConfigCompletion>,
    native_completion: Option<NativeCompletion>,
    native_begin_error: Option<HostError>,
    steer_completion: Option<ConfigCompletion>,
    steer_port: Option<Arc<Mutex<crate::acp::runtime::process_driver::ProcessLivePort>>>,
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
            terminal_gate: None,
            config_completion: None,
            native_completion: None,
            native_begin_error: None,
            steer_completion: None,
            steer_port: None,
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

    fn with_terminal_gate(mut self, gate: TerminalGate) -> Self {
        self.terminal_gate = Some(gate);
        self
    }

    fn with_config_completion(mut self, completion: ConfigCompletion) -> Self {
        self.config_completion = Some(completion);
        self
    }

    fn with_native_completion(mut self, completion: NativeCompletion) -> Self {
        self.native_completion = Some(completion);
        self
    }

    fn with_native_begin_error(mut self, error: HostError) -> Self {
        self.native_begin_error = Some(error);
        self
    }

    fn with_steer_completion(mut self, completion: ConfigCompletion) -> Self {
        self.steer_completion = Some(completion);
        self
    }

    fn with_steer_port(
        mut self,
        port: Arc<Mutex<crate::acp::runtime::process_driver::ProcessLivePort>>,
    ) -> Self {
        self.steer_port = Some(port);
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
    fn begin_native_rpc(
        &mut self,
        _intent: &crate::acp::goal::NativeRpcIntent,
        _deadline: Instant,
        completion: NativeRpcCompletionSink,
    ) -> Result<(), HostError> {
        self.push("beginNativeRpc");
        if let Some(error) = self.native_begin_error.clone() {
            return Err(error);
        }
        *self.native_completion.as_ref().unwrap().lock().unwrap() = Some(completion);
        Ok(())
    }

    fn begin_configure_session(
        &mut self,
        _intent: &crate::acp::session_config::ConfigIntent,
        _deadline: Instant,
        completion: ConfigCompletionSink,
    ) -> Result<(), HostError> {
        self.push("beginConfigureSession");
        match &self.config_completion {
            Some(retained) => {
                *retained.lock().unwrap() = Some(completion);
            }
            None => completion.complete(Ok(())),
        }
        Ok(())
    }

    fn abandon_configuration(&mut self, _deadline: Instant) {
        self.push("abandonConfiguration");
        if let Some(retained) = &self.config_completion {
            retained.lock().unwrap().take();
        }
    }

    fn resolve_permission(
        &mut self,
        _owner: &RuntimeOwner,
        _request_id: &crate::acp::contract::SageServerRequestId,
        _decision: crate::acp_protocol::permissions::PermissionDecision,
    ) -> Result<(), HostError> {
        self.push("resolvePermission");
        Ok(())
    }

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

    fn dispatch_steer(&mut self, _intent: &HostSteerIntent) -> Result<(), HostError> {
        self.push("dispatchSteer");
        if self.steer_completion.is_some() {
            std::thread::sleep(Duration::from_millis(750));
        }
        Ok(())
    }

    fn begin_steer(
        &mut self,
        intent: &HostSteerIntent,
        deadline: Instant,
        completion: SteerCompletionSink,
    ) -> Result<(), HostError> {
        self.push("dispatchSteer");
        if let Some(port) = &self.steer_port {
            return port
                .lock()
                .unwrap()
                .begin_steer(intent, deadline, completion);
        }
        match &self.steer_completion {
            Some(retained) => *retained.lock().unwrap() = Some(completion),
            None => completion.complete(Ok(())),
        }
        Ok(())
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
        if let Some(gate) = &self.terminal_gate {
            let (state, ready) = &**gate;
            let mut state = state.lock().unwrap();
            state.0 = true;
            ready.notify_all();
            while !state.1 {
                state = ready.wait(state).unwrap();
            }
        }
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

    fn shutdown(&mut self, _deadline: std::time::Instant) -> Result<(), HostError> {
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
            now_millis: sage_core::time::now_millis(),
        },
        access_mode: crate::acp_protocol::access_mode::SessionAccessMode::ReadOnly,
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
        now_millis: sage_core::time::now_millis(),
    }
}

fn steer(owner: &RuntimeOwner) -> HostSteerIntent {
    HostSteerIntent {
        owner: owner.clone(),
        turn_id: turn(),
        prompt: Prompt::new("prompt", "Change direction").unwrap(),
        client_message_id: crate::acp::contract::RequiredText::new("client", "steer-host").unwrap(),
    }
}

fn config(owner: &RuntimeOwner) -> crate::acp::session_config::ConfigIntent {
    crate::acp::session_config::ConfigIntent {
        owner: owner.clone(),
        expected_config_revision: crate::acp::session_config::ConfigRevision::first(),
        mutation: crate::acp::session_config::ConfigMutation::Mode(
            crate::acp::session_config::ConfigId::new("mode", "ask").unwrap(),
        ),
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
            now_millis: sage_core::time::now_millis(),
        },
    }
}

#[test]
fn stalled_worker_steer_admission_does_not_block_host_cancel() {
    let runtime = owner("project-a");
    let mailbox = Arc::new(crate::acp::runtime::process_driver::DriverMailbox::new().unwrap());
    let port = Arc::new(Mutex::new(
        crate::acp::runtime::process_driver::ProcessLivePort::new(
            runtime.clone(),
            mailbox,
            Arc::new(|_| {}),
        ),
    ));
    let engine = FakeEngine::new([PromptWrite::Complete(turn())]).with_steer_port(port);
    let observed = engine.clone();
    let host = Arc::new(RuntimeHost::start_with_timeout(engine, Duration::from_secs(1)).unwrap());
    host.activate(runtime.clone()).unwrap();
    host.send(prompt(&runtime)).unwrap();

    let pending = {
        let host = Arc::clone(&host);
        let intent = steer(&runtime);
        std::thread::spawn(move || host.steer(intent))
    };
    let started = Instant::now();
    while !observed.events().contains(&"dispatchSteer") {
        assert!(started.elapsed() < Duration::from_secs(1));
        std::thread::yield_now();
    }

    let control_started = Instant::now();
    assert_eq!(host.cancel(cancel(&runtime)), Ok(HostOutcome::CancelQueued));
    assert!(control_started.elapsed() < Duration::from_millis(250));
    assert_eq!(pending.join().unwrap(), Err(HostError::Intent));
    host.shutdown(RuntimeHost::close_deadline()).unwrap();
}

#[test]
fn pending_steer_does_not_block_controls_or_admit_a_second_steer() {
    let completion = Arc::new(Mutex::new(None));
    let engine = FakeEngine::new([PromptWrite::Complete(turn())])
        .with_steer_completion(Arc::clone(&completion));
    let observed = engine.clone();
    let host = Arc::new(RuntimeHost::start(engine).unwrap());
    let runtime = owner("project-a");
    host.activate(runtime.clone()).unwrap();
    host.send(prompt(&runtime)).unwrap();

    let pending = {
        let host = Arc::clone(&host);
        let intent = steer(&runtime);
        std::thread::spawn(move || host.steer(intent))
    };
    let started = Instant::now();
    while !observed.events().contains(&"dispatchSteer") {
        assert!(started.elapsed() < Duration::from_secs(1));
        std::thread::yield_now();
    }

    let second = {
        let host = Arc::clone(&host);
        let intent = steer(&runtime);
        std::thread::spawn(move || host.steer(intent))
    };
    let control_started = Instant::now();
    assert_eq!(second.join().unwrap(), Err(HostError::Intent));
    assert!(control_started.elapsed() < Duration::from_millis(250));
    assert_eq!(
        host.resolve_permission(
            runtime.clone(),
            crate::acp::contract::SageServerRequestId::parse("request-host").unwrap(),
            crate::acp_protocol::permissions::PermissionDecision::Deny,
        ),
        Ok(HostOutcome::PermissionResolved)
    );
    assert_eq!(host.cancel(cancel(&runtime)), Ok(HostOutcome::CancelQueued));
    assert!(control_started.elapsed() < Duration::from_millis(250));
    completion
        .lock()
        .unwrap()
        .take()
        .expect("pending Steer completion")
        .complete(Err(HostError::Intent));
    assert_eq!(pending.join().unwrap(), Err(HostError::Intent));
    host.shutdown(RuntimeHost::close_deadline()).unwrap();
}

#[test]
fn pending_configuration_does_not_block_permission_resolution() {
    let completion = Arc::new(Mutex::new(None));
    let engine = FakeEngine::new([PromptWrite::Complete(turn())])
        .with_config_completion(Arc::clone(&completion));
    let observed = engine.clone();
    let host = Arc::new(RuntimeHost::start(engine).unwrap());
    let runtime = owner("project-a");
    host.activate(runtime.clone()).unwrap();
    host.send(prompt(&runtime)).unwrap();

    let configuring = {
        let host = Arc::clone(&host);
        let intent = config(&runtime);
        std::thread::spawn(move || host.configure_session(intent))
    };
    let started = Instant::now();
    while completion.lock().unwrap().is_none() {
        assert!(started.elapsed() < Duration::from_secs(1));
        std::thread::yield_now();
    }

    assert_eq!(
        host.resolve_permission(
            runtime,
            crate::acp::contract::SageServerRequestId::parse("request-host").unwrap(),
            crate::acp_protocol::permissions::PermissionDecision::Deny,
        )
        .unwrap(),
        HostOutcome::PermissionResolved
    );
    completion.lock().unwrap().take().unwrap().complete(Ok(()));
    assert_eq!(
        configuring.join().unwrap().unwrap(),
        HostOutcome::SessionConfigured
    );
    assert!(
        observed
            .events()
            .ends_with(&["beginConfigureSession", "resolvePermission"])
    );
    host.shutdown(RuntimeHost::close_deadline()).unwrap();
}

#[test]
fn native_rpc_returns_a_typed_reply_without_coercing_it_to_an_outcome() {
    let completion = Arc::new(Mutex::new(None));
    let engine = FakeEngine::new([]).with_native_completion(Arc::clone(&completion));
    let host = Arc::new(RuntimeHost::start(engine).unwrap());
    let runtime = owner("project-a");
    host.activate(runtime.clone()).unwrap();
    let pending = {
        let host = Arc::clone(&host);
        std::thread::spawn(move || {
            host.native_rpc(crate::acp::goal::NativeRpcIntent {
                owner: runtime,
                request: crate::acp::goal::NativeRpcRequest::Goal(
                    crate::acp::goal::GoalAction::PauseExisting,
                ),
            })
        })
    };
    let started = Instant::now();
    while completion.lock().unwrap().is_none() {
        assert!(started.elapsed() < Duration::from_secs(1));
        std::thread::yield_now();
    }
    completion
        .lock()
        .unwrap()
        .take()
        .unwrap()
        .complete(Ok(crate::acp::goal::NativeRpcReply::GoalApplied));
    assert_eq!(
        pending.join().unwrap(),
        Ok(crate::acp::goal::NativeRpcReply::GoalApplied)
    );
    host.shutdown(RuntimeHost::close_deadline()).unwrap();
}

#[test]
fn native_rpc_rejects_a_foreign_owner_without_dispatch() {
    let completion = Arc::new(Mutex::new(None));
    let engine = FakeEngine::new([]).with_native_completion(Arc::clone(&completion));
    let observed = engine.clone();
    let host = RuntimeHost::start(engine).unwrap();
    host.activate(owner("project-a")).unwrap();
    assert_eq!(
        host.native_rpc(crate::acp::goal::NativeRpcIntent {
            owner: owner("project-b"),
            request: crate::acp::goal::NativeRpcRequest::Goal(
                crate::acp::goal::GoalAction::PauseExisting,
            ),
        }),
        Err(HostError::Intent)
    );
    assert!(completion.lock().unwrap().is_none());
    assert!(!observed.events().contains(&"beginNativeRpc"));
    host.shutdown(RuntimeHost::close_deadline()).unwrap();
}

#[test]
fn an_idle_park_waits_out_a_native_rpc_and_refuses_one_while_it_runs() {
    let completion = Arc::new(Mutex::new(None));
    let engine = FakeEngine::new([]).with_native_completion(Arc::clone(&completion));
    let observed = engine.clone();
    let host = Arc::new(RuntimeHost::start(engine).unwrap());
    let runtime = owner("project-a");
    host.activate(runtime.clone()).unwrap();
    let intent = crate::acp::goal::NativeRpcIntent {
        owner: runtime,
        request: crate::acp::goal::NativeRpcRequest::ForkSession,
    };
    let pending = {
        let host = Arc::clone(&host);
        let intent = intent.clone();
        std::thread::spawn(move || host.native_rpc(intent))
    };
    let started = Instant::now();
    while completion.lock().unwrap().is_none() {
        assert!(started.elapsed() < Duration::from_secs(1));
        std::thread::yield_now();
    }
    // The harness is mid-command: the runtime is not idle.
    assert!(!host.controls_settled());
    assert!(host.claim_idle_park().is_none());
    completion
        .lock()
        .unwrap()
        .take()
        .unwrap()
        .complete(Ok(crate::acp::goal::NativeRpcReply::Rejected));
    assert_eq!(
        pending.join().unwrap(),
        Ok(crate::acp::goal::NativeRpcReply::Rejected)
    );
    assert!(host.controls_settled());

    let claim = host.claim_idle_park().expect("a settled runtime can park");
    assert!(host.claim_idle_park().is_none(), "one park at a time");
    // A control that finds the park under way never reaches the harness.
    assert_eq!(host.native_rpc(intent.clone()), Err(HostError::State));
    assert_eq!(
        host.configure_session(config(&owner("project-a"))),
        Err(HostError::State)
    );
    let dispatched = |events: Vec<&'static str>| {
        events
            .iter()
            .filter(|event| **event == "beginNativeRpc" || **event == "beginConfigureSession")
            .count()
    };
    assert_eq!(dispatched(observed.events()), 1);
    // A park that does not go ahead lets controls through again.
    drop(claim);
    assert!(host.claim_idle_park().is_some());
    host.shutdown(RuntimeHost::close_deadline()).unwrap();
}

#[test]
fn late_native_rpc_completion_cannot_escape_its_deadline() {
    let completion = Arc::new(Mutex::new(None));
    let engine = FakeEngine::new([]).with_native_completion(Arc::clone(&completion));
    let host =
        Arc::new(RuntimeHost::start_with_timeout(engine, Duration::from_millis(100)).unwrap());
    let runtime = owner("project-a");
    host.without_timeout_override(|| host.activate(runtime.clone()))
        .unwrap();
    let pending = {
        let host = Arc::clone(&host);
        std::thread::spawn(move || {
            host.native_rpc(crate::acp::goal::NativeRpcIntent {
                owner: runtime,
                request: crate::acp::goal::NativeRpcRequest::Goal(
                    crate::acp::goal::GoalAction::PauseExisting,
                ),
            })
        })
    };
    let started = Instant::now();
    let retained = loop {
        if let Some(retained) = completion.lock().unwrap().take() {
            break retained;
        }
        assert!(started.elapsed() < Duration::from_secs(1));
        std::thread::yield_now();
    };
    assert_eq!(pending.join().unwrap(), Err(HostError::Deadline));
    retained.complete(Ok(crate::acp::goal::NativeRpcReply::GoalApplied));
    host.without_timeout_override(|| assert_eq!(host.state(), Ok(HostState::Recovery)));
    host.shutdown(RuntimeHost::close_deadline()).unwrap();
}

#[test]
fn a_passive_read_that_misses_its_deadline_is_abandoned_without_recovery() {
    let completion = Arc::new(Mutex::new(None));
    let engine = FakeEngine::new([]).with_native_completion(Arc::clone(&completion));
    let observed = engine.clone();
    let host = RuntimeHost::start_with_timeout(engine, Duration::from_millis(100)).unwrap();
    let runtime = owner("project-a");
    host.without_timeout_override(|| host.activate(runtime.clone()))
        .unwrap();
    assert_eq!(
        host.native_rpc(crate::acp::goal::NativeRpcIntent {
            owner: runtime,
            request: crate::acp::goal::NativeRpcRequest::UsageRead,
        }),
        Err(HostError::Deadline)
    );
    let started = Instant::now();
    while !observed.events().contains(&"abandonConfiguration") {
        assert!(started.elapsed() < Duration::from_secs(1));
        std::thread::yield_now();
    }
    completion
        .lock()
        .unwrap()
        .take()
        .unwrap()
        .complete(Ok(crate::acp::goal::NativeRpcReply::UsageRead));
    host.without_timeout_override(|| assert_eq!(host.state(), Ok(HostState::Idle)));
    host.shutdown(RuntimeHost::close_deadline()).unwrap();
}

#[test]
fn a_native_rpc_that_cannot_begin_in_time_recovers_only_when_not_passive() {
    for (request, settled) in [
        (
            crate::acp::goal::NativeRpcRequest::UsageRead,
            HostState::Idle,
        ),
        // Renaming the harness session is cosmetic: a slow one never
        // costs the chat its runtime.
        (
            crate::acp::goal::NativeRpcRequest::Control(
                crate::acp::session_control::SessionControl::SetTitle {
                    title: crate::acp::contract::RequiredText::new("sessionTitle", "Renamed")
                        .unwrap(),
                },
            ),
            HostState::Idle,
        ),
        // Listing MCP servers is a read; reloading them changes the session.
        (
            crate::acp::goal::NativeRpcRequest::Control(
                crate::acp::session_control::SessionControl::ReadMcpServers,
            ),
            HostState::Idle,
        ),
        // Reconnecting a server keeps its configuration; Claude Code may
        // still finish it after Sage stops waiting.
        (
            crate::acp::goal::NativeRpcRequest::Control(
                crate::acp::session_control::SessionControl::ReconnectMcpServer {
                    name: crate::acp::contract::RequiredText::new("serverName", "linear").unwrap(),
                },
            ),
            HostState::Idle,
        ),
        (
            crate::acp::goal::NativeRpcRequest::Control(
                crate::acp::session_control::SessionControl::ReloadMcpServers,
            ),
            HostState::Recovery,
        ),
        // An import read only borrows the lister's runtime and changes
        // nothing it holds; a child read stays bound to its own chat.
        (
            crate::acp::goal::NativeRpcRequest::Read(
                crate::acp::goal::NativeRead::ImportableSessions {
                    cursor: None,
                    excluded: Vec::new(),
                },
            ),
            HostState::Idle,
        ),
        (
            crate::acp::goal::NativeRpcRequest::Read(crate::acp::goal::NativeRead::ImportTurns {
                thread_id: "thread-cli".into(),
                cursor: None,
            }),
            HostState::Idle,
        ),
        (
            crate::acp::goal::NativeRpcRequest::Read(crate::acp::goal::NativeRead::ChildTurns {
                child_thread_id: "child-1".into(),
                parent_thread_id: "parent-1".into(),
                cursor: None,
            }),
            HostState::Recovery,
        ),
        (
            crate::acp::goal::NativeRpcRequest::Goal(crate::acp::goal::GoalAction::PauseExisting),
            HostState::Recovery,
        ),
    ] {
        let engine = FakeEngine::new([]).with_native_begin_error(HostError::Deadline);
        let host = RuntimeHost::start(engine).unwrap();
        let runtime = owner("project-a");
        host.activate(runtime.clone()).unwrap();
        assert_eq!(
            host.native_rpc(crate::acp::goal::NativeRpcIntent {
                owner: runtime,
                request,
            }),
            Err(HostError::Deadline)
        );
        assert_eq!(host.state(), Ok(settled));
        host.shutdown(RuntimeHost::close_deadline()).unwrap();
    }
}

#[test]
fn close_settles_pending_native_rpc_and_ignores_its_late_reply() {
    let completion = Arc::new(Mutex::new(None));
    let engine = FakeEngine::new([]).with_native_completion(Arc::clone(&completion));
    let host = Arc::new(RuntimeHost::start(engine).unwrap());
    let runtime = owner("project-a");
    host.activate(runtime.clone()).unwrap();
    let pending = {
        let host = Arc::clone(&host);
        let runtime = runtime.clone();
        std::thread::spawn(move || {
            host.native_rpc(crate::acp::goal::NativeRpcIntent {
                owner: runtime,
                request: crate::acp::goal::NativeRpcRequest::Goal(
                    crate::acp::goal::GoalAction::PauseExisting,
                ),
            })
        })
    };
    let started = Instant::now();
    let retained = loop {
        if let Some(retained) = completion.lock().unwrap().take() {
            break retained;
        }
        assert!(started.elapsed() < Duration::from_secs(1));
        std::thread::yield_now();
    };
    assert_eq!(host.close(close(&runtime)).unwrap(), HostOutcome::Closed);
    assert_eq!(pending.join().unwrap(), Err(HostError::Closed));
    retained.complete(Ok(crate::acp::goal::NativeRpcReply::GoalApplied));
    host.shutdown(RuntimeHost::close_deadline()).unwrap();
}

#[test]
fn pending_configuration_does_not_block_exact_turn_cancellation() {
    let completion = Arc::new(Mutex::new(None));
    let engine = FakeEngine::new([PromptWrite::Complete(turn())])
        .with_config_completion(Arc::clone(&completion));
    let observed = engine.clone();
    let host = Arc::new(RuntimeHost::start(engine).unwrap());
    let runtime = owner("project-a");
    host.activate(runtime.clone()).unwrap();
    host.send(prompt(&runtime)).unwrap();

    let configuring = {
        let host = Arc::clone(&host);
        let intent = config(&runtime);
        std::thread::spawn(move || host.configure_session(intent))
    };
    let started = Instant::now();
    while completion.lock().unwrap().is_none() {
        assert!(started.elapsed() < Duration::from_secs(1));
        std::thread::yield_now();
    }

    assert_eq!(
        host.cancel(cancel(&runtime)).unwrap(),
        HostOutcome::CancelQueued
    );
    completion.lock().unwrap().take().unwrap().complete(Ok(()));
    assert_eq!(
        configuring.join().unwrap().unwrap(),
        HostOutcome::SessionConfigured
    );
    assert!(observed.events().ends_with(&[
        "beginConfigureSession",
        "beginCancel",
        "dispatchCancel"
    ]));
    host.shutdown(RuntimeHost::close_deadline()).unwrap();
}

#[test]
fn close_abandons_pending_configuration_before_progressing() {
    let completion = Arc::new(Mutex::new(None));
    let engine = FakeEngine::new([]).with_config_completion(Arc::clone(&completion));
    let observed = engine.clone();
    let host = Arc::new(RuntimeHost::start(engine).unwrap());
    let runtime = owner("project-a");
    host.activate(runtime.clone()).unwrap();

    let configuring = {
        let host = Arc::clone(&host);
        let intent = config(&runtime);
        std::thread::spawn(move || host.configure_session(intent))
    };
    let started = Instant::now();
    while completion.lock().unwrap().is_none() {
        assert!(started.elapsed() < Duration::from_secs(1));
        std::thread::yield_now();
    }

    assert_eq!(host.close(close(&runtime)).unwrap(), HostOutcome::Closed);
    assert_eq!(configuring.join().unwrap(), Err(HostError::Closed));
    assert!(observed.events().ends_with(&[
        "beginConfigureSession",
        "abandonConfiguration",
        "beginClose",
        "convergeZero",
        "finalizeClose"
    ]));
    host.shutdown(RuntimeHost::close_deadline()).unwrap();
}

#[test]
fn shutdown_abandons_pending_configuration_before_progressing() {
    let completion = Arc::new(Mutex::new(None));
    let engine = FakeEngine::new([]).with_config_completion(Arc::clone(&completion));
    let observed = engine.clone();
    let host = Arc::new(RuntimeHost::start(engine).unwrap());
    let runtime = owner("project-a");
    host.activate(runtime.clone()).unwrap();

    let configuring = {
        let host = Arc::clone(&host);
        let intent = config(&runtime);
        std::thread::spawn(move || host.configure_session(intent))
    };
    let started = Instant::now();
    while completion.lock().unwrap().is_none() {
        assert!(started.elapsed() < Duration::from_secs(1));
        std::thread::yield_now();
    }

    host.shutdown(RuntimeHost::close_deadline()).unwrap();
    assert_eq!(configuring.join().unwrap(), Err(HostError::Closed));
    assert!(observed.events().starts_with(&[
        "activate",
        "beginConfigureSession",
        "abandonConfiguration"
    ]));
}

#[test]
fn configuration_deadline_wakes_actor_and_ignores_late_completion() {
    let completion = Arc::new(Mutex::new(None));
    let engine = FakeEngine::new([]).with_config_completion(Arc::clone(&completion));
    let observed = engine.clone();
    let host =
        Arc::new(RuntimeHost::start_with_timeout(engine, Duration::from_millis(25)).unwrap());
    let runtime = owner("project-a");
    host.without_timeout_override(|| host.activate(runtime.clone()))
        .unwrap();

    assert_eq!(
        host.configure_session(config(&runtime)),
        Err(HostError::Deadline)
    );
    host.without_timeout_override(|| {
        let settled = Instant::now();
        loop {
            match host.state() {
                Ok(HostState::Recovery) => break,
                Ok(state) => {
                    assert!(
                        settled.elapsed() < sage_test_support::test_wait::BUDGET,
                        "configuration deadline left host in {state:?}"
                    );
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("configuration deadline closed host: {error:?}"),
            }
        }
    });
    assert!(completion.lock().unwrap().is_none());
    assert_eq!(
        observed
            .events()
            .iter()
            .filter(|event| **event == "abandonConfiguration")
            .count(),
        1
    );
    host.shutdown(RuntimeHost::close_deadline()).unwrap();
}

#[test]
fn second_configuration_cannot_displace_the_pending_exact_owner() {
    let completion = Arc::new(Mutex::new(None));
    let engine = FakeEngine::new([]).with_config_completion(Arc::clone(&completion));
    let host = Arc::new(RuntimeHost::start(engine).unwrap());
    let runtime = owner("project-a");
    host.activate(runtime.clone()).unwrap();

    let configuring = {
        let host = Arc::clone(&host);
        let intent = config(&runtime);
        std::thread::spawn(move || host.configure_session(intent))
    };
    let started = Instant::now();
    while completion.lock().unwrap().is_none() {
        assert!(started.elapsed() < Duration::from_secs(1));
        std::thread::yield_now();
    }
    assert_eq!(
        host.configure_session(config(&runtime)),
        Err(HostError::State)
    );
    completion
        .lock()
        .unwrap()
        .take()
        .unwrap()
        .complete(Err(HostError::ConfigMutationRejected));
    assert_eq!(
        configuring.join().unwrap(),
        Err(HostError::ConfigMutationRejected)
    );
    assert_eq!(host.state().unwrap(), HostState::Idle);
    host.shutdown(RuntimeHost::close_deadline()).unwrap();
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
    host.shutdown(RuntimeHost::close_deadline()).unwrap();
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
    host.shutdown(RuntimeHost::close_deadline()).unwrap();
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
    host.shutdown(RuntimeHost::close_deadline()).unwrap();
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

    assert_eq!(
        host.send(prompt(&runtime)),
        Err(crate::acp::intent::PromptSendError::unknown(
            HostError::DeliveryUnknown
        ))
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
    host.shutdown(RuntimeHost::close_deadline()).unwrap();
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
    host.shutdown(RuntimeHost::close_deadline()).unwrap();
}

#[test]
fn failed_zero_byte_retry_enters_recovery_and_cannot_replay() {
    let engine =
        FakeEngine::new([PromptWrite::ZeroBytes(turn())]).with_retry_error(HostError::Unavailable);
    let observed = engine.clone();
    let host = RuntimeHost::start(engine).unwrap();
    let runtime = owner("project-a");
    host.activate(runtime.clone()).unwrap();

    assert_eq!(
        host.send(prompt(&runtime)),
        Err(crate::acp::intent::PromptSendError::unknown(
            HostError::Unavailable
        ))
    );
    assert_eq!(host.state().unwrap(), HostState::Recovery);
    assert_eq!(
        observed.events(),
        ["activate", "preparePrompt", "dispatchPrompt", "retryPrompt"]
    );
    host.shutdown(RuntimeHost::close_deadline()).unwrap();
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
        Err(crate::acp::intent::PromptSendError::unknown(
            HostError::RecoveryRequired
        ))
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
    host.shutdown(RuntimeHost::close_deadline()).unwrap();
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
    host.shutdown(RuntimeHost::close_deadline()).unwrap();
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

    assert_eq!(
        host.send(prompt(&runtime)),
        Err(crate::acp::intent::PromptSendError::unknown(
            HostError::Intent
        ))
    );
    assert_eq!(host.state().unwrap(), HostState::Recovery);
    host.shutdown(RuntimeHost::close_deadline()).unwrap();
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

    assert_eq!(
        host.send(prompt(&runtime)),
        Err(crate::acp::intent::PromptSendError::unknown(
            HostError::RetryExhausted
        ))
    );
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
    host.shutdown(RuntimeHost::close_deadline()).unwrap();
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
    host.shutdown(RuntimeHost::close_deadline()).unwrap();
}

#[test]
fn worker_terminal_notification_never_waits_for_the_host_callback() {
    use std::time::Duration;

    let gate = Arc::new((Mutex::new((false, false)), Condvar::new()));
    let engine =
        FakeEngine::new([PromptWrite::Complete(turn())]).with_terminal_gate(Arc::clone(&gate));
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

    let notification = host.try_notify_terminal(runtime, cursor).unwrap();
    let (state, ready) = &*gate;
    let state = state.lock().unwrap();
    let (mut state, timed) = ready
        .wait_timeout_while(state, Duration::from_secs(1), |state| !state.0)
        .unwrap();
    assert!(!timed.timed_out());
    state.1 = true;
    ready.notify_all();
    drop(state);

    notification.wait().unwrap();
    assert_eq!(host.state().unwrap(), HostState::Idle);
    assert_eq!(
        observed.events(),
        [
            "activate",
            "preparePrompt",
            "dispatchPrompt",
            "acceptTerminal"
        ]
    );
    host.shutdown(RuntimeHost::close_deadline()).unwrap();
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
    host.shutdown(RuntimeHost::close_deadline()).unwrap();
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
    host.shutdown(RuntimeHost::close_deadline()).unwrap();
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
    host.shutdown(RuntimeHost::close_deadline()).unwrap();
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
    host.shutdown(RuntimeHost::close_deadline()).unwrap();
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
            deadline: test_deadline(),
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
            deadline: test_deadline(),
        })
        .unwrap();
    let (shutdown_reply, shutdown_response) = mpsc::sync_channel(1);
    host.mailbox
        .try_push(Envelope {
            command: HostCommand::Shutdown,
            reply: shutdown_reply,
            deadline: test_deadline(),
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
                deadline: test_deadline(),
            })
            .unwrap();
    }
    let (reply, _response) = mpsc::sync_channel(1);
    assert!(matches!(
        mailbox.try_push(Envelope {
            command: HostCommand::ProjectSwitched(SageProjectId::parse("project-full").unwrap()),
            reply,
            deadline: test_deadline(),
        }),
        Err(HostError::Capacity)
    ));

    let (reply, _response) = mpsc::sync_channel(1);
    mailbox
        .try_push(Envelope {
            command: HostCommand::State,
            reply,
            deadline: test_deadline(),
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
        mailbox
            .try_push(Envelope {
                command,
                reply,
                deadline: test_deadline(),
            })
            .unwrap();
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
    use std::time::Duration;

    let gate = Arc::new((Mutex::new((false, false)), Condvar::new()));
    let engine = FakeEngine::new([]).with_prepare_gate(Arc::clone(&gate));
    let host = RuntimeHost::start_with_timeout(engine, Duration::from_millis(20)).unwrap();
    let runtime = owner("project-a");
    host.without_timeout_override(|| host.activate(runtime.clone()))
        .unwrap();

    assert_eq!(
        host.prepare_prompt(prompt(&runtime)),
        Err(HostError::Deadline)
    );
    assert_eq!(
        host.without_timeout_override(|| host.state()),
        Err(HostError::RecoveryRequired)
    );

    let (state, ready) = &*gate;
    let mut state = state.lock().unwrap();
    state.1 = true;
    ready.notify_all();
    drop(state);
    assert_eq!(host.shutdown(RuntimeHost::close_deadline()), Ok(()));
}

#[test]
fn command_deadlines_use_the_frozen_operation_classes() {
    use sage_core::limits::LIMITS;

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
    // A list read is answered at once; a reconnect waits on the server.
    let control = |control| {
        HostCommand::NativeRpc(crate::acp::goal::NativeRpcIntent {
            owner: runtime.clone(),
            request: crate::acp::goal::NativeRpcRequest::Control(control),
        })
        .deadline_millis()
    };
    use crate::acp::session_control::SessionControl;
    assert!(control(SessionControl::ReadMcpServers) < LIMITS.deadlines_ms.ordinary_rpc);
    assert_eq!(
        control(SessionControl::ReconnectMcpServer {
            name: crate::acp::contract::RequiredText::new("serverName", "linear").unwrap(),
        }),
        LIMITS.deadlines_ms.ordinary_rpc
    );
}

#[test]
fn a_prompt_carrying_the_largest_admitted_images_is_prepared() {
    use crate::acp::attached_context::files::images::{ImagePreview, PromptImage, PromptImages};

    let host = RuntimeHost::start(FakeEngine::new([])).unwrap();
    let runtime = owner("project-a");
    host.activate(runtime.clone()).unwrap();
    let mut intent = prompt(&runtime);
    // Two images at the echoed bound, each with a thumbnail, as a Codex or Pi
    // prompt can carry once admitted.
    let image = |bytes| PromptImage {
        media_type: crate::acp::intent::ImageMediaType::Png,
        bytes: vec![0; bytes],
        preview: ImagePreview::Thumbnail(
            crate::acp::intent::ImageMediaType::Png,
            vec![0; sage_core::limits::IMAGE_PREVIEW_BYTES],
        ),
    };
    let half = sage_core::limits::PROMPT_IMAGE_TOTAL_BYTES / 2;
    intent.command.prompt.images = PromptImages(Arc::new(vec![image(half), image(half)]));
    assert!(
        host.prepare_prompt(intent).is_ok(),
        "a prompt within the image bounds must reach the engine"
    );
    host.shutdown(RuntimeHost::close_deadline()).unwrap();
}

#[test]
fn a_prompt_refused_before_it_is_prepared_is_known_not_to_have_started() {
    use crate::acp::intent::PromptSendError;
    let host =
        RuntimeHost::start(FakeEngine::new([]).with_prepare_error(HostError::Capacity)).unwrap();
    let runtime = owner("project-a");
    host.activate(runtime.clone()).unwrap();
    // The chat offers the draft back only on this proof.
    assert_eq!(
        host.send(prompt(&runtime)),
        Err(PromptSendError::not_started(HostError::Capacity))
    );
    assert_eq!(host.state().unwrap(), HostState::Idle);
    host.shutdown(RuntimeHost::close_deadline()).unwrap();
}
