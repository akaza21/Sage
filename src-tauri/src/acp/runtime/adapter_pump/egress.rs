use super::*;

impl<A: HarnessAdapter, J: AdapterBatchJournal, R: ResourceMapper> AdapterPump<A, J, R> {
    pub(crate) fn queue_command(
        &mut self,
        command: AdapterCommand,
    ) -> Result<QueueDisposition, AdapterPumpError> {
        if matches!(command, AdapterCommand::Cancel) {
            return Err(AdapterPumpError::Intent);
        }
        self.ensure_live()?;
        let egress = self.adapter.command(command)?;
        self.enqueue(egress)
    }

    pub(crate) fn queue_cancel(
        &mut self,
        session_id: &SageAgentSessionId,
        epoch: RuntimeEpoch,
        turn_id: &SageTurnId,
        client_message_id: &RequiredText<512>,
    ) -> Result<QueueDisposition, AdapterPumpError> {
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
        let egress = self.adapter.command(AdapterCommand::Cancel)?;
        self.enqueue(egress)
    }

    pub(crate) fn queue_prompt(
        &mut self,
        prompt: crate::acp::intent::Prompt,
        client_message_id: &RequiredText<512>,
    ) -> Result<QueueDisposition, AdapterPumpError> {
        self.ensure_live()?;
        let active = &self.intent.active_turn;
        if client_message_id != &active.client_message_id || self.terminal.is_some() {
            return Err(AdapterPumpError::NoActiveTurn);
        }
        let egress = self.adapter.command(AdapterCommand::Prompt {
            prompt,
            client_message_id: client_message_id.clone(),
        })?;
        self.enqueue(egress)
    }

    pub(crate) fn flush_one<T: EgressTransport>(
        &mut self,
        transport: &mut T,
    ) -> Result<FlushDisposition, AdapterPumpError> {
        self.ensure_live()?;
        let Some(egress) = self.egress.pop() else {
            return Ok(FlushDisposition::Idle);
        };
        let mandatory = egress.token().is_mandatory();
        let written_permission = egress
            .token()
            .deny_request()
            .and_then(|(request, replay)| (!replay).then_some(request.to_owned()));
        let outcome = transport.write(egress.frame());
        let (resolution, disposition) = match outcome {
            FrameWriteOutcome::Complete => (EgressResolution::Written, FlushDisposition::Written),
            FrameWriteOutcome::ZeroByteFailure => (
                EgressResolution::WriteZeroBytes,
                FlushDisposition::ZeroBytes,
            ),
            FrameWriteOutcome::DeliveryUnknown { .. } => (
                EgressResolution::DeliveryUnknown,
                FlushDisposition::DeliveryUnknown,
            ),
        };
        let result = self.adapter.resolve_egress(egress.into_token(), resolution);
        let delivery_unknown = disposition == FlushDisposition::DeliveryUnknown;
        if delivery_unknown || (mandatory && disposition != FlushDisposition::Written) {
            self.teardown = true;
        }
        if result.is_err() {
            self.teardown = true;
            return Err(AdapterPumpError::TeardownRequired);
        }
        if disposition == FlushDisposition::Written
            && let Some(request) = written_permission
        {
            let Some(pending) = self.pending_permission.as_mut() else {
                self.teardown = true;
                return Err(AdapterPumpError::TeardownRequired);
            };
            if pending.intent.external_request_id.as_str() != request || pending.written {
                self.teardown = true;
                return Err(AdapterPumpError::TeardownRequired);
            }
            pending.written = true;
        }
        if delivery_unknown {
            Ok(disposition)
        } else if self.teardown {
            Err(AdapterPumpError::TeardownRequired)
        } else {
            Ok(disposition)
        }
    }

    pub(crate) async fn settle_written_permission(
        &mut self,
        now_millis: i64,
    ) -> Result<bool, AdapterPumpError> {
        let Some(pending) = self.pending_permission.as_ref() else {
            return Ok(false);
        };
        if !pending.written {
            return Ok(false);
        }
        let intent = pending.intent.clone();
        let revision = match self
            .journal
            .transition_permission(&intent, ServerRequestState::ResolvedDeclined, now_millis)
            .await
        {
            Ok(revision) => revision,
            Err(PumpJournalError::CommitUnknown | PumpJournalError::Rejected) => {
                self.teardown = true;
                return Err(AdapterPumpError::TeardownRequired);
            }
        };
        self.pending_permission = None;
        self.publish_revision(revision)?;
        Ok(true)
    }

    pub(super) fn enqueue(
        &mut self,
        value: AdapterEgress,
    ) -> Result<QueueDisposition, AdapterPumpError> {
        let token = value.token().clone();
        let mandatory = token.is_mandatory();
        let control = token.is_control();
        let bytes = value.frame().wire_len();
        let result = if control {
            self.egress.try_push_control_with(bytes, 0, || value)
        } else {
            self.egress.try_push_data_with(bytes, || value)
        };
        if result.is_err() {
            let resolved = self
                .adapter
                .resolve_egress(token, EgressResolution::QueueRejected);
            if mandatory || resolved.is_err() {
                self.teardown = true;
                return Err(AdapterPumpError::TeardownRequired);
            }
            return Err(AdapterPumpError::Backpressure);
        }
        if self
            .adapter
            .resolve_egress(token, EgressResolution::Queued)
            .is_err()
        {
            self.teardown = true;
            return Err(AdapterPumpError::TeardownRequired);
        }
        Ok(QueueDisposition::Queued)
    }
}
