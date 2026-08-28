use std::time::Instant;

use crate::acp::{
    contract::{SageServerRequestId, SageTurnId},
    runtime::permissions::PermissionDecision,
    runtime::{HostCancelIntent, HostError, HostPromptIntent, LiveRuntimePort, LiveWrite},
    scheduler::RuntimeOwner,
    store::{AdapterTurnCursor, CloseSessionCommand, SendObservation},
};

use super::{ProcessLivePort, WorkerCommand, WorkerReply};

impl LiveRuntimePort for ProcessLivePort {
    type ZeroProof = super::WorkerZeroProof;

    fn owner(&self) -> &RuntimeOwner {
        &self.owner
    }

    fn resolve_permission(
        &mut self,
        owner: &RuntimeOwner,
        request_id: &SageServerRequestId,
        decision: PermissionDecision,
    ) -> Result<(), HostError> {
        match self.call(WorkerCommand::ResolvePermission {
            owner: owner.clone(),
            request_id: request_id.clone(),
            decision,
        })? {
            WorkerReply::Unit => Ok(()),
            WorkerReply::Prepared(_) | WorkerReply::Write(_) | WorkerReply::Zero(_) => {
                Err(HostError::State)
            }
        }
    }

    fn prepare_prompt(&mut self, intent: &HostPromptIntent) -> Result<SageTurnId, HostError> {
        match self.call(WorkerCommand::PreparePrompt(intent.clone()))? {
            WorkerReply::Prepared(turn_id) => Ok(turn_id),
            WorkerReply::Write(_) | WorkerReply::Unit | WorkerReply::Zero(_) => {
                Err(HostError::State)
            }
        }
    }

    fn dispatch_prompt(
        &mut self,
        owner: &RuntimeOwner,
        turn_id: &SageTurnId,
    ) -> Result<LiveWrite, HostError> {
        match self.call(WorkerCommand::DispatchPrompt {
            owner: owner.clone(),
            turn_id: turn_id.clone(),
        })? {
            WorkerReply::Write(write) => Ok(write),
            WorkerReply::Prepared(_) | WorkerReply::Unit | WorkerReply::Zero(_) => {
                Err(HostError::State)
            }
        }
    }

    fn observe_send(
        &mut self,
        owner: &RuntimeOwner,
        turn_id: &SageTurnId,
        observation: SendObservation,
    ) -> Result<(), HostError> {
        match self.call(WorkerCommand::ObserveSend {
            owner: owner.clone(),
            turn_id: turn_id.clone(),
            observation,
        })? {
            WorkerReply::Unit => Ok(()),
            WorkerReply::Prepared(_) | WorkerReply::Write(_) | WorkerReply::Zero(_) => {
                Err(HostError::State)
            }
        }
    }

    fn begin_cancel(&mut self, intent: &HostCancelIntent) -> Result<(), HostError> {
        match self.call(WorkerCommand::BeginCancel(intent.clone()))? {
            WorkerReply::Unit => Ok(()),
            WorkerReply::Prepared(_) | WorkerReply::Write(_) | WorkerReply::Zero(_) => {
                Err(HostError::State)
            }
        }
    }

    fn dispatch_cancel(&mut self, intent: &HostCancelIntent) -> Result<(), HostError> {
        match self.call(WorkerCommand::DispatchCancel(intent.clone()))? {
            WorkerReply::Unit => Ok(()),
            WorkerReply::Prepared(_) | WorkerReply::Write(_) | WorkerReply::Zero(_) => {
                Err(HostError::State)
            }
        }
    }

    fn accept_terminal(&mut self, cursor: &AdapterTurnCursor) -> Result<(), HostError> {
        match self.call(WorkerCommand::AcceptTerminal {
            owner: self.owner.clone(),
            cursor: cursor.clone(),
        })? {
            WorkerReply::Unit => Ok(()),
            WorkerReply::Prepared(_) | WorkerReply::Write(_) | WorkerReply::Zero(_) => {
                Err(HostError::State)
            }
        }
    }

    fn accept_latest_completed(&mut self) -> Result<(), HostError> {
        match self.call(WorkerCommand::AcceptLatestCompleted)? {
            WorkerReply::Unit => Ok(()),
            WorkerReply::Prepared(_) | WorkerReply::Write(_) | WorkerReply::Zero(_) => {
                Err(HostError::State)
            }
        }
    }

    fn begin_close(&mut self, command: &CloseSessionCommand) -> Result<(), HostError> {
        match self.call(WorkerCommand::BeginClose(command.clone()))? {
            WorkerReply::Unit => Ok(()),
            WorkerReply::Prepared(_) | WorkerReply::Write(_) | WorkerReply::Zero(_) => {
                Err(HostError::State)
            }
        }
    }

    fn wait_zero(&mut self, owner: &RuntimeOwner) -> Result<Self::ZeroProof, HostError> {
        match self.call(WorkerCommand::WaitZero {
            owner: owner.clone(),
        })? {
            WorkerReply::Zero(proof) => Ok(proof),
            WorkerReply::Prepared(_) | WorkerReply::Write(_) | WorkerReply::Unit => {
                Err(HostError::State)
            }
        }
    }

    fn finalize_close(
        &mut self,
        command: &CloseSessionCommand,
        proof: Self::ZeroProof,
    ) -> Result<(), (HostError, Self::ZeroProof)> {
        let retained = proof.clone();
        match self.call(WorkerCommand::FinalizeClose {
            command: command.clone(),
            proof,
        }) {
            Ok(WorkerReply::Unit) => Ok(()),
            Ok(WorkerReply::Prepared(_) | WorkerReply::Write(_) | WorkerReply::Zero(_)) => {
                Err((HostError::State, retained))
            }
            Err(error) => Err((error, retained)),
        }
    }

    fn begin_park(&mut self) -> Result<(), HostError> {
        match self.call(WorkerCommand::BeginPark)? {
            WorkerReply::Unit => Ok(()),
            WorkerReply::Prepared(_) | WorkerReply::Write(_) | WorkerReply::Zero(_) => {
                Err(HostError::State)
            }
        }
    }

    fn park_zero(&mut self, deadline: Instant) -> Result<(), HostError> {
        match self.call_until(
            WorkerCommand::WaitZero {
                owner: self.owner.clone(),
            },
            deadline,
        ) {
            Ok(WorkerReply::Zero(_)) => match self.call_until(
                WorkerCommand::ParkAfterZero {
                    owner: self.owner.clone(),
                },
                deadline,
            ) {
                Ok(WorkerReply::Unit) | Err(HostError::Closed | HostError::State) => Ok(()),
                Ok(_) => Err(HostError::State),
                Err(error) => Err(error),
            },
            Err(HostError::Closed | HostError::State) => Ok(()),
            Ok(_) => Err(HostError::State),
            Err(error) => Err(error),
        }
    }

    fn shutdown(&mut self, deadline: Instant) -> Result<(), HostError> {
        match self.call_until(WorkerCommand::Shutdown, deadline) {
            Ok(WorkerReply::Unit) | Err(HostError::Closed) => self.join_if_managed(deadline),
            Err(error) => self.finish_shutdown_join(error, deadline),
            Ok(_) => Err(HostError::State),
        }
    }
}

impl ProcessLivePort {
    fn finish_shutdown_join(
        &mut self,
        error: HostError,
        deadline: Instant,
    ) -> Result<(), HostError> {
        match error {
            HostError::Deadline | HostError::Closed => match self.managed_runner.as_ref() {
                Some(runner) => runner.join_before(deadline),
                None => Err(error),
            },
            error => Err(error),
        }
    }

    fn join_if_managed(&mut self, deadline: Instant) -> Result<(), HostError> {
        match self.managed_runner.as_ref() {
            Some(runner) => runner.join_before(deadline),
            None => Ok(()),
        }
    }
}
