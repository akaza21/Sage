use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use crate::acp::{store::AdapterTurnCursor, transcript_quote::Prompt};
use sage_acp_protocol::{
    contract::{
        ClientIntentId, ProjectGeneration, ProjectionRevision, RuntimeEpoch, SageAgentSessionId,
        SageChatId, SageProjectId, SageTurnId,
    },
    lifecycle::{DeliveryState, TurnState},
};

use super::*;

#[derive(Clone)]
struct Trace(Arc<Mutex<Vec<String>>>);

impl Trace {
    fn push(&self, value: impl Into<String>) {
        self.0.lock().unwrap().push(value.into());
    }

    fn values(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

struct Port {
    owner: RuntimeOwner,
    turn: SageTurnId,
    writes: VecDeque<LiveWrite>,
    finalize_failures: VecDeque<HostError>,
    prepared_prompt: Option<Prompt>,
    trace: Trace,
}

impl LiveRuntimePort for Port {
    type ZeroProof = u64;

    fn owner(&self) -> &RuntimeOwner {
        &self.owner
    }

    fn prepare_prompt(&mut self, intent: &HostPromptIntent) -> Result<SageTurnId, HostError> {
        self.trace.push("worker:prepare+bind");
        self.prepared_prompt = Some(intent.command.prompt.clone());
        Ok(self.turn.clone())
    }

    fn dispatch_prompt(
        &mut self,
        owner: &RuntimeOwner,
        turn: &SageTurnId,
    ) -> Result<LiveWrite, HostError> {
        if owner != &self.owner || turn != &self.turn || self.prepared_prompt.is_none() {
            return Err(HostError::Intent);
        }
        self.trace.push(format!("worker:write:{}", turn.as_str()));
        self.writes.pop_front().ok_or(HostError::Unavailable)
    }

    fn observe_send(
        &mut self,
        owner: &RuntimeOwner,
        turn_id: &SageTurnId,
        value: SendObservation,
    ) -> Result<(), HostError> {
        if owner != &self.owner || turn_id != &self.turn {
            return Err(HostError::Intent);
        }
        self.trace.push(format!("worker:observe:{value:?}"));
        Ok(())
    }

    fn begin_cancel(&mut self, _: &HostCancelIntent) -> Result<(), HostError> {
        self.trace.push("worker:cancel-commit");
        Ok(())
    }

    fn dispatch_cancel(&mut self, _: &HostCancelIntent) -> Result<(), HostError> {
        self.trace.push("worker:cancel-write");
        Ok(())
    }

    fn accept_terminal(&mut self, _: &AdapterTurnCursor) -> Result<(), HostError> {
        self.trace.push("worker:terminal");
        Ok(())
    }

    fn begin_close(&mut self, _: &CloseSessionCommand) -> Result<(), HostError> {
        self.trace.push("worker:close-commit");
        if self.prepared_prompt.is_some() {
            self.trace.push("worker:cancel-commit");
            self.trace.push("worker:cancel-write");
        }
        self.trace.push("worker:terminate");
        Ok(())
    }

    fn wait_zero(&mut self, _: &RuntimeOwner) -> Result<u64, HostError> {
        self.trace.push("worker:zero");
        Ok(7)
    }

    fn finalize_close(
        &mut self,
        _: &CloseSessionCommand,
        proof: u64,
    ) -> Result<(), (HostError, u64)> {
        self.trace.push(format!("worker:finalize:{proof}"));
        if let Some(error) = self.finalize_failures.pop_front() {
            return Err((error, proof));
        }
        Ok(())
    }

    fn shutdown(&mut self, _deadline: std::time::Instant) -> Result<(), HostError> {
        self.trace.push("worker:shutdown");
        Ok(())
    }
}

fn owner() -> RuntimeOwner {
    RuntimeOwner {
        project_id: SageProjectId::parse("project-live").unwrap(),
        chat_id: SageChatId::parse("chat-live").unwrap(),
        session_id: SageAgentSessionId::parse("session-live").unwrap(),
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
            client_intent_id: ClientIntentId::parse("intent-live").unwrap(),
            prompt: Prompt::new("prompt", "hello").unwrap(),
            now_millis: 10,
        },
        access_mode: sage_acp_protocol::access_mode::SessionAccessMode::ReadOnly,
    }
}

fn close(owner: &RuntimeOwner) -> HostCloseIntent {
    HostCloseIntent {
        command: CloseSessionCommand {
            project_id: owner.project_id.clone(),
            chat_id: owner.chat_id.clone(),
            expected_projection_revision: ProjectionRevision::ZERO,
            client_intent_id: ClientIntentId::parse("intent-close").unwrap(),
            session_id: owner.session_id.clone(),
            runtime_epoch: owner.runtime_epoch,
            now_millis: 20,
        },
    }
}

fn engine(writes: impl IntoIterator<Item = LiveWrite>) -> (LiveRuntimeEngine<Port>, Trace) {
    let trace = Trace(Arc::new(Mutex::new(Vec::new())));
    (
        LiveRuntimeEngine::new(Port {
            owner: owner(),
            turn: SageTurnId::parse("turn-live").unwrap(),
            writes: writes.into_iter().collect(),
            finalize_failures: VecDeque::new(),
            prepared_prompt: None,
            trace: trace.clone(),
        }),
        trace,
    )
}

#[test]
fn failed_finalize_retains_the_exact_zero_proof_for_explicit_recovery() {
    let (mut engine, trace) = engine([]);
    engine
        .port
        .finalize_failures
        .push_back(HostError::RecoveryRequired);
    let host = super::super::RuntimeHost::start(engine).unwrap();
    let owner = owner();
    let intent = close(&owner);
    host.activate(owner).unwrap();
    host.begin_close(intent.clone()).unwrap();

    assert_eq!(
        host.finish_close(intent.clone()),
        Err(HostError::RecoveryRequired)
    );
    assert_eq!(
        host.recover_close(intent).unwrap(),
        super::super::HostOutcome::Closed
    );
    assert_eq!(
        trace.values(),
        vec![
            "worker:close-commit",
            "worker:terminate",
            "worker:zero",
            "worker:finalize:7",
            "worker:finalize:7",
        ]
    );
    host.shutdown(crate::acp::runtime::RuntimeHost::close_deadline())
        .unwrap();
}

#[test]
fn zero_byte_retry_prepares_once_binds_once_and_reuses_store_minted_id() {
    let (engine, trace) = engine([LiveWrite::ZeroBytes, LiveWrite::Complete]);
    let host = super::super::RuntimeHost::start(engine).unwrap();
    let owner = owner();
    host.activate(owner.clone()).unwrap();
    assert_eq!(
        host.send(prompt(&owner)).unwrap(),
        super::super::HostOutcome::PromptAccepted
    );
    assert_eq!(
        trace.values(),
        vec![
            "worker:prepare+bind",
            "worker:write:turn-live",
            "worker:observe:ZeroBytes",
            "worker:write:turn-live",
            "worker:observe:CompleteBytes",
        ]
    );
    host.shutdown(crate::acp::runtime::RuntimeHost::close_deadline())
        .unwrap();
}

#[test]
fn possible_bytes_commit_delivery_unknown_without_retry() {
    let (engine, trace) = engine([LiveWrite::PossibleBytes, LiveWrite::Complete]);
    let host = super::super::RuntimeHost::start(engine).unwrap();
    let owner = owner();
    host.activate(owner.clone()).unwrap();
    assert_eq!(
        host.send(prompt(&owner)),
        Err(crate::acp::prompt_send::PromptSendError::unknown(
            HostError::DeliveryUnknown
        ))
    );
    assert_eq!(
        trace.values(),
        vec![
            "worker:prepare+bind",
            "worker:write:turn-live",
            "worker:observe:PossibleBytes",
        ]
    );
    host.shutdown(crate::acp::runtime::RuntimeHost::close_deadline())
        .unwrap();
}

#[test]
fn close_commits_before_termination_and_finalizes_only_after_zero_proof() {
    let (engine, trace) = engine([]);
    let host = super::super::RuntimeHost::start(engine).unwrap();
    let owner = owner();
    host.activate(owner.clone()).unwrap();
    assert_eq!(
        host.close(close(&owner)).unwrap(),
        super::super::HostOutcome::Closed
    );
    assert_eq!(
        trace.values(),
        vec![
            "worker:close-commit",
            "worker:terminate",
            "worker:zero",
            "worker:finalize:7",
        ]
    );
    host.shutdown(crate::acp::runtime::RuntimeHost::close_deadline())
        .unwrap();
}

#[test]
fn active_close_orders_session_cancel_adapter_control_and_termination_before_zero() {
    let (engine, trace) = engine([LiveWrite::Complete]);
    let host = super::super::RuntimeHost::start(engine).unwrap();
    let owner = owner();
    host.activate(owner.clone()).unwrap();
    host.send(prompt(&owner)).unwrap();
    host.close(close(&owner)).unwrap();

    assert_eq!(
        trace.values(),
        vec![
            "worker:prepare+bind",
            "worker:write:turn-live",
            "worker:observe:CompleteBytes",
            "worker:close-commit",
            "worker:cancel-commit",
            "worker:cancel-write",
            "worker:terminate",
            "worker:zero",
            "worker:finalize:7",
        ]
    );
    host.shutdown(crate::acp::runtime::RuntimeHost::close_deadline())
        .unwrap();
}

#[test]
fn terminal_cursor_is_forwarded_to_the_same_worker_owner() {
    let (engine, trace) = engine([LiveWrite::Complete]);
    let host = super::super::RuntimeHost::start(engine).unwrap();
    let owner = owner();
    host.activate(owner.clone()).unwrap();
    host.send(prompt(&owner)).unwrap();
    host.accept_terminal(
        owner,
        AdapterTurnCursor::test_only(
            SageTurnId::parse("turn-live").unwrap(),
            TurnState::Completed,
            DeliveryState::Correlated,
            sage_acp_protocol::contract::EntityRevision::new(3).unwrap(),
            ProjectionRevision::new(4).unwrap(),
        ),
    )
    .unwrap();
    assert!(trace.values().contains(&"worker:terminal".to_owned()));
    host.shutdown(crate::acp::runtime::RuntimeHost::close_deadline())
        .unwrap();
}
