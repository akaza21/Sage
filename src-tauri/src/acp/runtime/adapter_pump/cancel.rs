use super::*;

impl<A: HarnessAdapter, J: AdapterBatchJournal, R: ResourceMapper> AdapterPump<A, J, R> {
    pub(crate) async fn queue_cancelled_permissions(
        &mut self,
        session_id: &SageAgentSessionId,
        epoch: RuntimeEpoch,
        turn_id: &SageTurnId,
        client_message_id: &RequiredText<512>,
        now_millis: i64,
    ) -> Result<usize, AdapterPumpError> {
        self.validate_cancel_owner(session_id, epoch, turn_id, client_message_id)?;
        let limit = LIMITS.queues.server_requests_per_session as usize;
        let mut batch = crate::acp::bindings::adapter::AdapterBatchBuilder::new();
        let mut intents = Vec::new();
        intents
            .try_reserve_exact(limit)
            .map_err(|_| AdapterPumpError::BatchCapacity)?;
        for pending in self
            .pending_permissions
            .iter()
            .filter(|pending| pending.decision.is_none())
        {
            if intents.len() >= limit {
                self.teardown = true;
                return Err(AdapterPumpError::TeardownRequired);
            }
            batch.push_egress(self.adapter.cancel_permission(&pending.intent.request_id)?)?;
            intents.push(pending.intent.clone());
        }
        let (_, egress) = batch.finish().into_parts();
        for intent in &intents {
            let revision = self
                .journal
                .transition_permission(intent, ServerRequestState::Cancelled, now_millis)
                .await
                .map_err(|_| {
                    self.teardown = true;
                    AdapterPumpError::TeardownRequired
                })?;
            self.publish_revision(revision)?;
        }
        for value in &egress {
            if self.adapter.commit_mandatory_egress(value.token()).is_err() {
                self.teardown = true;
                return Err(AdapterPumpError::TeardownRequired);
            }
        }
        for value in egress {
            self.enqueue(value)?;
        }
        self.pending_permissions
            .retain(|pending| pending.decision.is_some());
        Ok(intents.len())
    }

    pub(crate) fn queue_cancel_notification(
        &mut self,
        session_id: &SageAgentSessionId,
        epoch: RuntimeEpoch,
        turn_id: &SageTurnId,
        client_message_id: &RequiredText<512>,
    ) -> Result<QueueDisposition, AdapterPumpError> {
        self.validate_cancel_owner(session_id, epoch, turn_id, client_message_id)?;
        let egress = self.adapter.command(AdapterCommand::Cancel)?;
        self.enqueue(egress)
    }

    fn validate_cancel_owner(
        &self,
        session_id: &SageAgentSessionId,
        epoch: RuntimeEpoch,
        turn_id: &SageTurnId,
        client_message_id: &RequiredText<512>,
    ) -> Result<(), AdapterPumpError> {
        self.ensure_live()?;
        let active = &self.intent.active_turn;
        if session_id != &self.intent.session_id
            || epoch != self.intent.runtime_epoch
            || turn_id != &active.turn_id
            || client_message_id != &active.client_message_id
            || self.terminal.is_some()
        {
            return Err(AdapterPumpError::NoActiveTurn);
        }
        Ok(())
    }
}
