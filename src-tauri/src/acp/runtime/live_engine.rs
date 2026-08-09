use crate::acp::{
    contract::SageTurnId,
    scheduler::RuntimeOwner,
    store::{AdapterTurnCursor, CloseSessionCommand, SendObservation},
};
use std::time::Instant;

use super::{
    HostCancelIntent, HostCloseIntent, HostError, HostPromptIntent, PromptWrite, RuntimeEngine,
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

    fn owner(&self) -> &RuntimeOwner;
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
    fn observe_send(
        &mut self,
        owner: &RuntimeOwner,
        turn_id: &SageTurnId,
        observation: SendObservation,
    ) -> Result<(), HostError>;
    fn begin_cancel(&mut self, intent: &HostCancelIntent) -> Result<(), HostError>;
    fn dispatch_cancel(&mut self, intent: &HostCancelIntent) -> Result<(), HostError>;
    fn accept_terminal(&mut self, cursor: &AdapterTurnCursor) -> Result<(), HostError>;
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
        match self.port.dispatch_prompt(owner, turn_id)? {
            LiveWrite::Complete => {
                self.port
                    .observe_send(owner, turn_id, SendObservation::CompleteBytes)?;
                Ok(PromptWrite::Complete(turn_id.clone()))
            }
            LiveWrite::ZeroBytes => {
                self.port
                    .observe_send(owner, turn_id, SendObservation::ZeroBytes)?;
                Ok(PromptWrite::ZeroBytes(turn_id.clone()))
            }
            LiveWrite::PossibleBytes => Ok(PromptWrite::PossibleBytes(turn_id.clone())),
        }
    }
}

impl<P: LiveRuntimePort> RuntimeEngine for LiveRuntimeEngine<P> {
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

    fn dispatch_cancel(&mut self, intent: &HostCancelIntent) -> Result<(), HostError> {
        self.port.dispatch_cancel(intent)
    }

    fn accept_terminal(&mut self, cursor: &AdapterTurnCursor) -> Result<(), HostError> {
        self.port.accept_terminal(cursor)
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

    fn shutdown(&mut self, deadline: Instant) -> Result<(), HostError> {
        self.port.shutdown(deadline)
    }
}

#[cfg(test)]
mod tests;
