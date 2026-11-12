use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU8, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use crate::acp::{intent::PromptSendError, scheduler::RuntimeOwner, store::AdapterTurnCursor};

use super::super::bounded_thread::{BoundedThread, ThreadWaitError};
pub(crate) use super::types::TerminalNotification;

use super::types::HostState;
use super::{
    actor::Actor,
    mailbox::{Envelope, HostMailbox},
    types::{
        HostCancelIntent, HostCloseIntent, HostCommand, HostError, HostOutcome, HostPromptIntent,
        HostReply, RuntimeEngine, RuntimeShutdownRecovery,
    },
};

pub(crate) struct RuntimeHost {
    pub(super) mailbox: Arc<HostMailbox>,
    thread: Mutex<Option<BoundedThread>>,
    pub(crate) lifecycle: AtomicU8,
    shutdown_recovery: Option<Arc<dyn RuntimeShutdownRecovery>>,
    controls: host_control_gate::ControlGate,
    #[cfg(any(test, all(windows, feature = "acp-fixture")))]
    timeout_override: Option<TimeoutOverride>,
    #[cfg(test)]
    timeout_override_suspended: std::sync::atomic::AtomicBool,
    #[cfg(test)]
    pub(crate) last_shutdown_phase: Mutex<&'static str>,
}

#[cfg(any(test, all(windows, feature = "acp-fixture")))]
#[derive(Clone, Copy)]
enum TimeoutOverride {
    #[cfg(test)]
    Duration(Duration),
    #[cfg_attr(
        not(all(windows, feature = "acp-fixture")),
        allow(dead_code, reason = "only the Windows fixture lane sets a deadline")
    )]
    Deadline {
        operation: Instant,
        cleanup: Instant,
    },
}

#[path = "host_session_config.rs"]
mod host_session_config;

#[path = "host_control_gate.rs"]
mod host_control_gate;

#[path = "host_dispatch.rs"]
mod host_dispatch;

impl RuntimeHost {
    #[cfg(test)]
    pub(crate) fn start<E: RuntimeEngine>(engine: E) -> Result<Self, HostError> {
        Self::start_inner(engine, None, None)
    }

    pub(crate) fn start_with_shutdown_recovery<E, R>(
        engine: E,
        shutdown_recovery: Arc<R>,
    ) -> Result<Self, HostError>
    where
        E: RuntimeEngine,
        R: RuntimeShutdownRecovery + 'static,
    {
        let shutdown_recovery: Arc<dyn RuntimeShutdownRecovery> = shutdown_recovery;
        Self::start_inner(engine, None, Some(shutdown_recovery))
    }

    #[cfg(test)]
    pub(crate) fn start_with_timeout<E: RuntimeEngine>(
        engine: E,
        timeout: Duration,
    ) -> Result<Self, HostError> {
        Self::start_inner(engine, Some(TimeoutOverride::Duration(timeout)), None)
    }

    #[cfg(test)]
    pub(crate) fn start_with_timeout_and_shutdown_recovery<E, R>(
        engine: E,
        timeout: Duration,
        shutdown_recovery: Arc<R>,
    ) -> Result<Self, HostError>
    where
        E: RuntimeEngine,
        R: RuntimeShutdownRecovery + 'static,
    {
        let shutdown_recovery: Arc<dyn RuntimeShutdownRecovery> = shutdown_recovery;
        Self::start_inner(
            engine,
            Some(TimeoutOverride::Duration(timeout)),
            Some(shutdown_recovery),
        )
    }

    #[cfg(all(test, windows, feature = "acp-fixture"))]
    pub(crate) fn start_with_deadline<E: RuntimeEngine>(
        engine: E,
        operation: Instant,
        cleanup: Instant,
    ) -> Result<Self, HostError> {
        Self::start_inner(
            engine,
            Some(TimeoutOverride::Deadline { operation, cleanup }),
            None,
        )
    }

    #[cfg(all(windows, feature = "acp-fixture"))]
    pub(crate) fn start_with_deadline_and_shutdown_recovery<E, R>(
        engine: E,
        operation: Instant,
        cleanup: Instant,
        shutdown_recovery: Arc<R>,
    ) -> Result<Self, HostError>
    where
        E: RuntimeEngine,
        R: RuntimeShutdownRecovery + 'static,
    {
        let shutdown_recovery: Arc<dyn RuntimeShutdownRecovery> = shutdown_recovery;
        Self::start_inner(
            engine,
            Some(TimeoutOverride::Deadline { operation, cleanup }),
            Some(shutdown_recovery),
        )
    }

    fn start_inner<E: RuntimeEngine>(
        engine: E,
        #[cfg(any(test, all(windows, feature = "acp-fixture")))] timeout_override: Option<
            TimeoutOverride,
        >,
        #[cfg(not(any(test, all(windows, feature = "acp-fixture"))))] _timeout_override: Option<
            Duration,
        >,
        shutdown_recovery: Option<Arc<dyn RuntimeShutdownRecovery>>,
    ) -> Result<Self, HostError> {
        let mailbox = Arc::new(HostMailbox::new()?);
        let actor_mailbox = Arc::clone(&mailbox);
        let thread = BoundedThread::spawn("sage-acp-runtime-host", move || {
            Actor::new(engine).run(actor_mailbox)
        })
        .map_err(|_| HostError::Unavailable)?;
        Ok(Self {
            mailbox,
            thread: Mutex::new(Some(thread)),
            lifecycle: AtomicU8::new(HOST_RUNNING),
            shutdown_recovery,
            controls: host_control_gate::ControlGate::default(),
            #[cfg(any(test, all(windows, feature = "acp-fixture")))]
            timeout_override,
            #[cfg(test)]
            timeout_override_suspended: std::sync::atomic::AtomicBool::new(false),
            #[cfg(test)]
            last_shutdown_phase: Mutex::new("none"),
        })
    }

    #[cfg(test)]
    pub(crate) fn without_timeout_override<T>(&self, body: impl FnOnce() -> T) -> T {
        self.timeout_override_suspended
            .store(true, Ordering::Release);
        let outcome = body();
        self.timeout_override_suspended
            .store(false, Ordering::Release);
        outcome
    }

    pub(crate) fn activate(&self, owner: RuntimeOwner) -> Result<HostOutcome, HostError> {
        self.outcome(HostCommand::Activate(owner))
    }

    pub(crate) fn resolve_permission(
        &self,
        owner: RuntimeOwner,
        request_id: crate::acp::contract::SageServerRequestId,
        decision: crate::acp::runtime::permissions::PermissionDecision,
    ) -> Result<HostOutcome, HostError> {
        self.outcome(HostCommand::ResolvePermission {
            owner,
            request_id,
            decision,
        })
    }

    pub(crate) fn prepare_prompt(
        &self,
        intent: HostPromptIntent,
    ) -> Result<crate::acp::contract::SageTurnId, HostError> {
        match self.submit(HostCommand::PreparePrompt(intent))? {
            HostReply::Prepared(turn_id) => Ok(turn_id),
            HostReply::Outcome(_) | HostReply::NativeRpc(_) | HostReply::State(_) => {
                Err(HostError::State)
            }
        }
    }

    pub(crate) fn dispatch_prepared(
        &self,
        owner: RuntimeOwner,
        turn_id: crate::acp::contract::SageTurnId,
    ) -> Result<HostOutcome, HostError> {
        self.outcome(HostCommand::DispatchPrompt { owner, turn_id })
    }

    /// Prepares and dispatches a prompt. A prompt the mailbox or the idle
    /// actor refused before preparing it is known not to have started, so
    /// its draft can be offered again; any later failure is unknown.
    pub(crate) fn send(
        &self,
        intent: HostPromptIntent,
    ) -> Result<HostOutcome, PromptSendError<HostError>> {
        let started = Instant::now();
        let owner = intent.owner();
        let turn_id = self.prepare_prompt(intent).map_err(|error| {
            crate::acp::runtime::prompt_diagnostics::failed(
                "host.prepare",
                started,
                &error,
                self.lifecycle.load(Ordering::Acquire),
            );
            match error {
                HostError::Capacity | HostError::Intent => PromptSendError::not_started(error),
                _ => PromptSendError::unknown(error),
            }
        })?;
        let prepared_ms = started.elapsed().as_millis();
        self.dispatch_prepared(owner, turn_id)
            .inspect_err(|error| {
                crate::acp::runtime::prompt_diagnostics::failed(
                    "host.dispatch",
                    started,
                    error,
                    (
                        "prepared_ms",
                        prepared_ms,
                        "lifecycle",
                        self.lifecycle.load(Ordering::Acquire),
                    ),
                );
            })
            .map_err(PromptSendError::unknown)
    }

    pub(crate) fn cancel(&self, intent: HostCancelIntent) -> Result<HostOutcome, HostError> {
        self.outcome(HostCommand::Cancel(intent))
    }

    pub(crate) fn steer(
        &self,
        intent: super::types::HostSteerIntent,
    ) -> Result<HostOutcome, HostError> {
        self.outcome(HostCommand::Steer(intent))
    }

    #[cfg(test)]
    pub(crate) fn accept_terminal(
        &self,
        owner: RuntimeOwner,
        cursor: AdapterTurnCursor,
    ) -> Result<HostOutcome, HostError> {
        self.outcome(HostCommand::Terminal { owner, cursor })
    }

    #[cfg(test)]
    pub(crate) fn project_switched(
        &self,
        project: crate::acp::contract::SageProjectId,
    ) -> Result<HostOutcome, HostError> {
        self.outcome(HostCommand::ProjectSwitched(project))
    }

    /// Project replacement is admitted under the caller's single absolute
    /// deadline; unlike `project_switched`, this never starts a fresh ordinary
    /// RPC budget after discovery or P-02 settlement consumed time.
    pub(crate) fn project_switched_until(
        &self,
        project: crate::acp::contract::SageProjectId,
        deadline: Instant,
    ) -> Result<HostOutcome, HostError> {
        self.outcome_until(HostCommand::ProjectSwitched(project), deadline)
    }

    pub(crate) fn state(&self) -> Result<HostState, HostError> {
        match self.submit(HostCommand::State)? {
            HostReply::State(state) => Ok(state),
            HostReply::Outcome(_) | HostReply::NativeRpc(_) | HostReply::Prepared(_) => {
                Err(HostError::State)
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn begin_close(&self, intent: HostCloseIntent) -> Result<HostOutcome, HostError> {
        self.outcome(HostCommand::BeginClose(intent))
    }

    #[cfg(test)]
    pub(crate) fn finish_close(&self, intent: HostCloseIntent) -> Result<HostOutcome, HostError> {
        self.outcome(HostCommand::FinishClose(intent))
    }

    pub(crate) fn recover_close(&self, intent: HostCloseIntent) -> Result<HostOutcome, HostError> {
        self.outcome(HostCommand::RecoverClose(intent))
    }

    pub(crate) fn close(&self, intent: HostCloseIntent) -> Result<HostOutcome, HostError> {
        let started = Instant::now();
        let budget = Duration::from_millis(crate::acp::limits::LIMITS.deadlines_ms.explicit_close);
        self.outcome_with_timeout(HostCommand::BeginClose(intent.clone()), budget)?;
        self.outcome_with_timeout(
            HostCommand::FinishClose(intent),
            budget.saturating_sub(started.elapsed()),
        )
    }

    /// Joins the host actor, then consumes the managed runner's retained
    /// recovery and returns supervisor-minted zero-process evidence. Plain
    /// in-memory hosts have no native recovery authority and return `None`.
    pub(crate) fn shutdown_recovery_before(
        &self,
        deadline: Instant,
    ) -> Result<Option<crate::acp::process::ZeroProcessToken>, HostError> {
        self.shutdown_before(deadline)?;
        self.shutdown_recovery
            .as_ref()
            .map(|recovery| recovery.shutdown_recovery_before(deadline))
            .transpose()
    }

    pub(crate) fn has_shutdown_recovery(&self) -> bool {
        self.shutdown_recovery.is_some()
    }

    #[cfg(test)]
    pub(crate) fn is_awaiting_exit(&self) -> bool {
        self.lifecycle.load(Ordering::Acquire) == HOST_AWAITING_EXIT
    }

    /// Stops a host that was assembled for an activation which never became
    /// a usable runtime. Closing the mailbox prevents the actor from issuing
    /// any new process command; joining is the ownership barrier.
    pub(crate) fn abandon_startup(&self) -> Result<(), HostError> {
        self.mailbox.close();
        self.lifecycle.store(HOST_AWAITING_EXIT, Ordering::Release);
        self.join_with_timeout(self.cleanup_timeout())
    }
}

impl Drop for RuntimeHost {
    fn drop(&mut self) {
        if self.thread.get_mut().is_ok_and(|thread| thread.is_none()) {
            return;
        }
        let started = Instant::now();
        let budget = self.cleanup_timeout();
        if self.lifecycle.load(Ordering::Acquire) == HOST_RUNNING
            && self.outcome(HostCommand::Shutdown).is_ok()
        {
            self.lifecycle
                .store(HOST_SHUTDOWN_ACKNOWLEDGED, Ordering::Release);
        } else {
            self.mailbox.close();
        }
        let _ = self.join_with_timeout(budget.saturating_sub(started.elapsed()));
    }
}

pub(crate) const HOST_RUNNING: u8 = 0;
pub(crate) const HOST_AWAITING_EXIT: u8 = 1;
pub(crate) const HOST_SHUTDOWN_ACKNOWLEDGED: u8 = 2;
pub(crate) const HOST_JOINED: u8 = 3;
