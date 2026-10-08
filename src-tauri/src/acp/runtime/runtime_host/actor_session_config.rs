use super::super::{
    mailbox::{Envelope, HostMailbox},
    types::{
        ConfigCompletion, ConfigCompletionSink, HostError, HostOutcome, HostReply, HostState,
        NativeRpcCompletion, NativeRpcCompletionSink, PendingHostConfig, RuntimeEngine,
    },
};
use super::Actor;

impl<E: RuntimeEngine> Actor<E> {
    pub(super) fn begin_configure_session(
        &mut self,
        envelope: Envelope,
        mailbox: &std::sync::Arc<HostMailbox>,
    ) {
        let Envelope {
            command,
            reply,
            deadline,
        } = envelope;
        let (owner, native_rpc) = match &command {
            super::super::types::HostCommand::ConfigureSession(intent) => {
                (intent.owner.clone(), false)
            }
            super::super::types::HostCommand::NativeRpc(intent) => (intent.owner.clone(), true),
            _ => {
                let _ = reply.send(Err(HostError::State));
                return;
            }
        };
        if std::time::Instant::now() >= deadline {
            let _ = reply.send(Err(HostError::Deadline));
            return;
        }
        if self.pending_config.is_some() {
            let _ = reply.send(Err(HostError::State));
            return;
        }
        if !matches!(self.state, HostState::Idle | HostState::Running)
            || !self.matches_owner(&owner)
        {
            let _ = reply.send(Err(HostError::Intent));
            return;
        }
        let Some(operation_id) = self.next_config_operation.checked_add(1) else {
            let _ = reply.send(Err(HostError::Capacity));
            return;
        };
        self.next_config_operation = operation_id;
        self.pending_config = Some(PendingHostConfig {
            operation_id,
            owner,
            reply,
            deadline,
            native_rpc,
        });
        let begin = match command {
            super::super::types::HostCommand::ConfigureSession(intent) => {
                let completion_owner = self.pending_config.as_ref().unwrap().owner.clone();
                let mailbox = std::sync::Arc::clone(mailbox);
                let completion = ConfigCompletionSink::new(move |result| {
                    mailbox.complete_config(ConfigCompletion {
                        operation_id,
                        owner: completion_owner,
                        result,
                    });
                });
                self.engine
                    .begin_configure_session(&intent, deadline, completion)
            }
            super::super::types::HostCommand::NativeRpc(intent) => {
                let completion_owner = self.pending_config.as_ref().unwrap().owner.clone();
                let mailbox = std::sync::Arc::clone(mailbox);
                let completion = NativeRpcCompletionSink::new(move |result| {
                    mailbox.complete_native_rpc(NativeRpcCompletion {
                        operation_id,
                        owner: completion_owner,
                        result,
                    });
                });
                self.engine.begin_native_rpc(&intent, deadline, completion)
            }
            _ => unreachable!(),
        };
        if let Err(error) = begin {
            self.fail_pending_config(error);
        }
    }

    pub(super) fn complete_config(&mut self, completion: ConfigCompletion) {
        let matches = self.pending_config.as_ref().is_some_and(|pending| {
            !pending.native_rpc
                && pending.operation_id == completion.operation_id
                && pending.owner == completion.owner
        });
        if !matches {
            return;
        }
        let pending = self.pending_config.take().expect("matched pending config");
        if std::time::Instant::now() >= pending.deadline {
            self.engine.abandon_configuration(pending.deadline);
            self.apply_config_error(&HostError::Deadline);
            let _ = pending.reply.send(Err(HostError::Deadline));
            return;
        }
        if let Err(error) = &completion.result {
            self.apply_config_error(error);
        }
        let reply = completion
            .result
            .map(|()| HostReply::Outcome(HostOutcome::SessionConfigured));
        let _ = pending.reply.send(reply);
    }

    pub(super) fn complete_native_rpc(&mut self, completion: NativeRpcCompletion) {
        let matches = self.pending_config.as_ref().is_some_and(|pending| {
            pending.native_rpc
                && pending.operation_id == completion.operation_id
                && pending.owner == completion.owner
        });
        if !matches {
            return;
        }
        let pending = self.pending_config.take().expect("matched native RPC");
        if std::time::Instant::now() >= pending.deadline {
            self.engine.abandon_configuration(pending.deadline);
            self.apply_config_error(&HostError::Deadline);
            let _ = pending.reply.send(Err(HostError::Deadline));
            return;
        }
        if let Err(error) = &completion.result {
            self.apply_config_error(error);
        }
        let _ = pending
            .reply
            .send(completion.result.map(HostReply::NativeRpc));
    }

    pub(super) fn fail_pending_config(&mut self, error: HostError) {
        self.apply_config_error(&error);
        self.settle_pending_config(error);
    }

    pub(super) fn settle_pending_config(&mut self, error: HostError) {
        let Some(pending) = self.pending_config.take() else {
            return;
        };
        let _ = pending.reply.send(Err(error));
    }

    fn apply_config_error(&mut self, error: &HostError) {
        if matches!(
            error,
            HostError::Unavailable
                | HostError::DeliveryUnknown
                | HostError::RecoveryRequired
                | HostError::Deadline
                | HostError::Closed
        ) {
            self.state = HostState::Recovery;
        }
    }
}
