use std::time::Instant;

use crate::acp::{
    contract::{SageServerRequestId, SageTurnId},
    runtime::permissions::PermissionDecision,
    runtime::{
        ConfigCompletionSink, HostCancelIntent, HostError, HostPromptIntent, HostSteerIntent,
        LiveRuntimePort, LiveWrite, SteerCompletionSink,
    },
    scheduler::RuntimeOwner,
    store::{AdapterTurnCursor, CloseSessionCommand, SendObservation},
};

use super::{ProcessLivePort, WorkerCommand, WorkerReply};

impl LiveRuntimePort for ProcessLivePort {
    type ZeroProof = super::WorkerZeroProof;
    fn begin_native_rpc(
        &mut self,
        intent: &crate::acp::goal::NativeRpcIntent,
        deadline: Instant,
        completion: ConfigCompletionSink,
    ) -> Result<(), HostError> {
        let response = self
            .mailbox
            .submit_worker_until(
                WorkerCommand::NativeRpc {
                    intent: intent.clone(),
                    completion,
                },
                deadline,
            )
            .map_err(super::support::map_mailbox)?;
        match response.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(Ok(WorkerReply::Unit)) => Ok(()),
            Ok(Ok(_)) => Err(HostError::State),
            Ok(Err(error)) => Err(error),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(HostError::Deadline),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err(HostError::Closed),
        }
    }
    fn begin_configure_session(
        &mut self,
        intent: &crate::acp::session_config::ConfigIntent,
        deadline: Instant,
        completion: ConfigCompletionSink,
    ) -> Result<(), HostError> {
        let response = self
            .mailbox
            .submit_worker_until(
                WorkerCommand::ConfigureSession {
                    intent: intent.clone(),
                    completion,
                },
                deadline,
            )
            .map_err(super::support::map_mailbox)?;
        // The worker owns completion settlement from this point. The bounded
        // worker reply reports only whether the begin phase was admitted and
        // may therefore be consumed synchronously without awaiting JSON-RPC.
        match response.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(Ok(WorkerReply::Unit)) => Ok(()),
            Ok(Ok(_)) => Err(HostError::State),
            Ok(Err(error)) => Err(error),
            // Configuration uses one original absolute deadline. Once admitted,
            // the live runner owns deadline settlement and recovery; this begin
            // caller must not turn a scheduling tie into process termination.
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(HostError::Deadline),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err(HostError::Closed),
        }
    }

    fn abandon_configuration(&mut self, deadline: Instant) {
        ProcessLivePort::abandon_configuration(self, deadline);
    }

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

    fn dispatch_steer(&mut self, intent: &HostSteerIntent) -> Result<(), HostError> {
        let (sender, response) = std::sync::mpsc::sync_channel(1);
        let deadline = Instant::now() + std::time::Duration::from_secs(10);
        self.begin_steer(
            intent,
            deadline,
            SteerCompletionSink::new(move |result| {
                let _ = sender.send(result);
            }),
        )?;
        response
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|_| HostError::SteerOutcomeUnknown)?
    }

    fn begin_steer(
        &mut self,
        intent: &HostSteerIntent,
        deadline: Instant,
        completion: SteerCompletionSink,
    ) -> Result<(), HostError> {
        self.mailbox
            .submit_worker_until(
                WorkerCommand::Steer {
                    intent: intent.clone(),
                    completion,
                },
                deadline,
            )
            .map_err(super::support::map_mailbox)?;
        Ok(())
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
            Ok(WorkerReply::Zero(_)) => {
                #[cfg(test)]
                if std::mem::take(&mut self.finish_park_failure) {
                    return Err(HostError::Deadline);
                }
                match self.call_until(
                    WorkerCommand::ParkAfterZero {
                        owner: self.owner.clone(),
                    },
                    deadline,
                ) {
                    Ok(WorkerReply::Unit) | Err(HostError::Closed | HostError::State) => Ok(()),
                    Ok(_) => Err(HostError::State),
                    Err(error) => Err(error),
                }
            }
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
    pub(crate) fn abandon_configuration(&self, deadline: Instant) {
        let command = WorkerCommand::AbandonConfiguration {
            owner: self.owner.clone(),
        };
        if deadline > Instant::now() {
            if let Ok(response) = self.mailbox.submit_worker_until(command, deadline) {
                let _ = response.recv_timeout(deadline.saturating_duration_since(Instant::now()));
            }
        } else {
            // At the operation deadline, queue a best-effort exact-owner release
            // on the control lane without minting a second wait budget. The live
            // runner's own deadline wake still settles the retained callback.
            let _ = self.mailbox.submit_worker(command);
        }
    }

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
