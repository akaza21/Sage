use crate::acp::{
    contract::{SageServerRequestId, SageTurnId},
    runtime::permissions::PermissionDecision,
    scheduler::RuntimeOwner,
    store::{AdapterTurnCursor, CloseSessionCommand, SendObservation},
};
use std::time::Instant;

use super::{
    ConfigCompletionSink, HostCancelIntent, HostCloseIntent, HostError, HostPromptIntent,
    HostSteerIntent, NativeRpcCompletionSink, PromptWrite, RuntimeEngine, SteerCompletionSink,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LiveWrite {
    Complete,
    ZeroBytes,
    PossibleBytes,
}

/// A bounded request/response port to the one process worker that owns the
/// adapter, its process tree, `SessionTreeLease`, `SendCoordinator`, and active
/// Turn dispatch. The worker must not poll and must retain all ownership until
/// a typed zero-process handoff.
pub(crate) trait LiveRuntimePort: Send + 'static {
    type ZeroProof: Send + 'static;

    fn begin_native_rpc(
        &mut self,
        _intent: &crate::acp::goal::NativeRpcIntent,
        _deadline: Instant,
        completion: NativeRpcCompletionSink,
    ) -> Result<(), HostError> {
        completion.complete(Err(HostError::State));
        Ok(())
    }

    fn begin_configure_session(
        &mut self,
        _intent: &crate::acp::session_config::ConfigIntent,
        _deadline: Instant,
        completion: ConfigCompletionSink,
    ) -> Result<(), HostError> {
        completion.complete(Err(HostError::State));
        Ok(())
    }
    fn abandon_configuration(&mut self, _deadline: Instant) {}
    fn owner(&self) -> &RuntimeOwner;
    fn resolve_permission(
        &mut self,
        _owner: &RuntimeOwner,
        _request_id: &SageServerRequestId,
        _decision: PermissionDecision,
    ) -> Result<(), HostError> {
        Err(HostError::State)
    }
    /// Commits the Turn and binds its exact `ActiveTurnIntent` to the idle
    /// adapter before returning. No stdin bytes may be written in this phase.
    fn prepare_prompt(&mut self, intent: &HostPromptIntent) -> Result<SageTurnId, HostError>;
    /// Commits send-start immediately before the worker writes this exact
    /// prompt. `client_message_id` is derived from the store-minted Sage Turn.
    fn dispatch_prompt(
        &mut self,
        owner: &RuntimeOwner,
        turn_id: &SageTurnId,
    ) -> Result<LiveWrite, HostError>;
    fn dispatch_steer(&mut self, _intent: &HostSteerIntent) -> Result<(), HostError> {
        Err(HostError::State)
    }
    fn begin_steer(
        &mut self,
        intent: &HostSteerIntent,
        _deadline: Instant,
        completion: SteerCompletionSink,
    ) -> Result<(), HostError> {
        completion.complete(self.dispatch_steer(intent));
        Ok(())
    }
    fn observe_send(
        &mut self,
        owner: &RuntimeOwner,
        turn_id: &SageTurnId,
        observation: SendObservation,
    ) -> Result<(), HostError>;
    fn begin_cancel(&mut self, intent: &HostCancelIntent) -> Result<(), HostError>;
    fn dispatch_cancel(&mut self, intent: &HostCancelIntent) -> Result<(), HostError>;
    fn accept_terminal(&mut self, cursor: &AdapterTurnCursor) -> Result<(), HostError>;
    fn accept_latest_completed(&mut self) -> Result<(), HostError> {
        Ok(())
    }
    /// Commits the session close intent, durably begins cancellation for any
    /// correlated active Turn, dispatches adapter cancellation, and only then
    /// requests process-tree termination.
    fn begin_close(&mut self, command: &CloseSessionCommand) -> Result<(), HostError>;
    fn wait_zero(&mut self, owner: &RuntimeOwner) -> Result<Self::ZeroProof, HostError>;
    fn finalize_close(
        &mut self,
        command: &CloseSessionCommand,
        proof: Self::ZeroProof,
    ) -> Result<(), (HostError, Self::ZeroProof)>;
    fn begin_park(&mut self) -> Result<(), HostError> {
        Err(HostError::State)
    }
    fn park_zero(&mut self, _deadline: Instant) -> Result<(), HostError> {
        Ok(())
    }
    fn shutdown(&mut self, deadline: Instant) -> Result<(), HostError>;
}

pub(crate) struct LiveRuntimeEngine<P: LiveRuntimePort> {
    port: P,
    zero_proof: Option<P::ZeroProof>,
}

impl<P: LiveRuntimePort> LiveRuntimeEngine<P> {
    pub(crate) fn new(port: P) -> Self {
        Self {
            port,
            zero_proof: None,
        }
    }

    fn write(
        &mut self,
        owner: &RuntimeOwner,
        turn_id: &SageTurnId,
    ) -> Result<PromptWrite, HostError> {
        let started = Instant::now();
        match self
            .port
            .dispatch_prompt(owner, turn_id)
            .inspect_err(|error| {
                super::prompt_diagnostics::failed(
                    "engine.write",
                    started,
                    error,
                    owner.runtime_epoch,
                );
            })? {
            LiveWrite::Complete => {
                self.port
                    .observe_send(owner, turn_id, SendObservation::CompleteBytes)
                    .inspect_err(|error| {
                        super::prompt_diagnostics::failed(
                            "engine.observe_complete",
                            started,
                            error,
                            owner.runtime_epoch,
                        );
                    })?;
                Ok(PromptWrite::Complete(turn_id.clone()))
            }
            LiveWrite::ZeroBytes => {
                self.port
                    .observe_send(owner, turn_id, SendObservation::ZeroBytes)
                    .inspect_err(|error| {
                        super::prompt_diagnostics::failed(
                            "engine.observe_zero",
                            started,
                            error,
                            owner.runtime_epoch,
                        );
                    })?;
                Ok(PromptWrite::ZeroBytes(turn_id.clone()))
            }
            LiveWrite::PossibleBytes => Ok(PromptWrite::PossibleBytes(turn_id.clone())),
        }
    }
}

impl<P: LiveRuntimePort> RuntimeEngine for LiveRuntimeEngine<P> {
    fn begin_native_rpc(
        &mut self,
        intent: &crate::acp::goal::NativeRpcIntent,
        deadline: Instant,
        completion: NativeRpcCompletionSink,
    ) -> Result<(), HostError> {
        if self.port.owner() != &intent.owner || self.zero_proof.is_some() {
            return Err(HostError::Intent);
        }
        self.port.begin_native_rpc(intent, deadline, completion)
    }
    fn begin_configure_session(
        &mut self,
        intent: &crate::acp::session_config::ConfigIntent,
        deadline: Instant,
        completion: ConfigCompletionSink,
    ) -> Result<(), HostError> {
        if self.port.owner() != &intent.owner || self.zero_proof.is_some() {
            return Err(HostError::Intent);
        }
        self.port
            .begin_configure_session(intent, deadline, completion)
    }
    fn abandon_configuration(&mut self, deadline: Instant) {
        self.port.abandon_configuration(deadline);
    }
    fn resolve_permission(
        &mut self,
        owner: &RuntimeOwner,
        request_id: &SageServerRequestId,
        decision: PermissionDecision,
    ) -> Result<(), HostError> {
        if self.port.owner() != owner || self.zero_proof.is_some() {
            return Err(HostError::Intent);
        }
        self.port.resolve_permission(owner, request_id, decision)
    }
    fn activate(&mut self, owner: &RuntimeOwner) -> Result<(), HostError> {
        if self.port.owner() == owner {
            Ok(())
        } else {
            Err(HostError::Intent)
        }
    }

    fn prepare_prompt(&mut self, intent: &HostPromptIntent) -> Result<SageTurnId, HostError> {
        self.port.prepare_prompt(intent)
    }

    fn dispatch_prompt(
        &mut self,
        owner: &RuntimeOwner,
        turn_id: &SageTurnId,
    ) -> Result<PromptWrite, HostError> {
        self.write(owner, turn_id)
    }

    fn retry_prompt(
        &mut self,
        owner: &RuntimeOwner,
        turn_id: &SageTurnId,
    ) -> Result<PromptWrite, HostError> {
        self.write(owner, turn_id)
    }

    fn record_delivery_unknown(
        &mut self,
        owner: &RuntimeOwner,
        turn_id: &SageTurnId,
    ) -> Result<(), HostError> {
        self.port
            .observe_send(owner, turn_id, SendObservation::PossibleBytes)
    }

    fn record_retry_exhausted(
        &mut self,
        owner: &RuntimeOwner,
        turn_id: &SageTurnId,
    ) -> Result<(), HostError> {
        self.port
            .observe_send(owner, turn_id, SendObservation::ProcessLost)
    }

    fn begin_cancel(&mut self, intent: &HostCancelIntent) -> Result<(), HostError> {
        self.port.begin_cancel(intent)
    }

    fn dispatch_steer(&mut self, intent: &HostSteerIntent) -> Result<(), HostError> {
        self.port.dispatch_steer(intent)
    }

    fn begin_steer(
        &mut self,
        intent: &HostSteerIntent,
        deadline: Instant,
        completion: SteerCompletionSink,
    ) -> Result<(), HostError> {
        if self.port.owner() != &intent.owner || self.zero_proof.is_some() {
            return Err(HostError::Intent);
        }
        self.port.begin_steer(intent, deadline, completion)
    }

    fn dispatch_cancel(&mut self, intent: &HostCancelIntent) -> Result<(), HostError> {
        self.port.dispatch_cancel(intent)
    }

    fn accept_terminal(&mut self, cursor: &AdapterTurnCursor) -> Result<(), HostError> {
        self.port.accept_terminal(cursor)
    }

    fn accept_latest_completed(&mut self) -> Result<(), HostError> {
        self.port.accept_latest_completed()
    }

    fn begin_close(&mut self, intent: &HostCloseIntent) -> Result<(), HostError> {
        if self.port.owner() != &intent.owner() || self.zero_proof.is_some() {
            return Err(HostError::Intent);
        }
        self.port.begin_close(&intent.command)
    }

    fn converge_zero(&mut self, intent: &HostCloseIntent) -> Result<(), HostError> {
        if self.port.owner() != &intent.owner() {
            return Err(HostError::Intent);
        }
        if self.zero_proof.is_some() {
            return Ok(());
        }
        self.zero_proof = Some(self.port.wait_zero(&intent.owner())?);
        Ok(())
    }

    fn finalize_close(&mut self, intent: &HostCloseIntent) -> Result<(), HostError> {
        let proof = self.zero_proof.take().ok_or(HostError::RecoveryRequired)?;
        match self.port.finalize_close(&intent.command, proof) {
            Ok(()) => Ok(()),
            Err((error, proof)) => {
                self.zero_proof = Some(proof);
                Err(error)
            }
        }
    }

    fn begin_park(&mut self) -> Result<(), HostError> {
        match self.port.begin_park() {
            Ok(()) | Err(HostError::Closed | HostError::State) => Ok(()),
            Err(error) => Err(error),
        }
    }

    fn park_zero(&mut self, deadline: Instant) -> Result<(), HostError> {
        match self.port.park_zero(deadline) {
            Ok(()) | Err(HostError::Closed | HostError::State) => Ok(()),
            Err(error) => Err(error),
        }
    }

    fn shutdown(&mut self, deadline: Instant) -> Result<(), HostError> {
        self.port.shutdown(deadline)
    }
}

#[cfg(test)]
mod tests;
