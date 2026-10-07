use super::super::{
    mailbox::{Envelope, HostMailbox},
    types::{
        HostCommand, HostError, HostOutcome, HostReply, HostState, PendingHostSteer, RuntimeEngine,
        SteerCompletion, SteerCompletionSink,
    },
};
use super::Actor;

impl<E: RuntimeEngine> Actor<E> {
    pub(super) fn begin_steer(
        &mut self,
        envelope: Envelope,
        mailbox: &std::sync::Arc<HostMailbox>,
    ) {
        let Envelope {
            command,
            reply,
            deadline,
        } = envelope;
        if self
            .pending_steer
            .as_ref()
            .is_some_and(|pending| std::time::Instant::now() >= pending.deadline)
        {
            self.settle_pending_steer(HostError::SteerOutcomeUnknown);
        }
        let HostCommand::Steer(intent) = command else {
            let _ = reply.send(Err(HostError::State));
            return;
        };
        if std::time::Instant::now() >= deadline {
            let _ = reply.send(Err(HostError::Deadline));
            return;
        }
        if self.pending_steer.is_some()
            || self.state != HostState::Running
            || !self.matches_owner(&intent.owner)
            || self.active_turn.as_ref() != Some(&intent.turn_id)
            || self.pending_prompt
            || self.pending_cancel.is_some()
        {
            let _ = reply.send(Err(HostError::Intent));
            return;
        }
        let Some(operation_id) = self.next_steer_operation.checked_add(1) else {
            let _ = reply.send(Err(HostError::Capacity));
            return;
        };
        self.next_steer_operation = operation_id;
        let owner = intent.owner.clone();
        let completion_owner = owner.clone();
        let mailbox = std::sync::Arc::clone(mailbox);
        let completion = SteerCompletionSink::new(move |result| {
            mailbox.complete_steer(SteerCompletion {
                operation_id,
                owner: completion_owner,
                result,
            });
        });
        self.pending_steer = Some(PendingHostSteer {
            operation_id,
            owner,
            reply,
            deadline,
        });
        if let Err(error) = self.engine.begin_steer(&intent, deadline, completion) {
            self.settle_pending_steer(error);
        }
    }

    pub(super) fn complete_steer(&mut self, completion: SteerCompletion) {
        let matches = self.pending_steer.as_ref().is_some_and(|pending| {
            pending.operation_id == completion.operation_id && pending.owner == completion.owner
        });
        if !matches {
            return;
        }
        let pending = self.pending_steer.take().expect("matched pending Steer");
        let result = if std::time::Instant::now() >= pending.deadline {
            Err(HostError::SteerOutcomeUnknown)
        } else {
            completion.result
        };
        if matches!(
            result,
            Err(HostError::Unavailable | HostError::RecoveryRequired | HostError::Closed)
        ) {
            self.state = HostState::Recovery;
        }
        let _ = pending
            .reply
            .send(result.map(|()| HostReply::Outcome(HostOutcome::SteerQueued)));
    }

    pub(super) fn settle_pending_steer(&mut self, error: HostError) {
        let Some(pending) = self.pending_steer.take() else {
            return;
        };
        let _ = pending.reply.send(Err(error));
    }
}
