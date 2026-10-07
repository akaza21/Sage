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

    /// The service has already committed the user's decision before it reaches
    /// the worker. Ask changes only durable state, while Allow and Deny retain
    /// the returned response in the bounded control lane for this exact
    /// previously-journaled request.
    pub(crate) async fn resolve_permission(
        &mut self,
        request_id: &SageServerRequestId,
        decision: crate::acp::runtime::permissions::PermissionDecision,
        now_millis: i64,
    ) -> Result<(), AdapterPumpError> {
        self.ensure_live()?;
        let Some(pending) = self
            .pending_permissions
            .iter()
            .find(|pending| &pending.intent.request_id == request_id)
        else {
            return Err(AdapterPumpError::Intent);
        };
        if pending.decision.is_some() {
            return Err(AdapterPumpError::Intent);
        }
        if decision == crate::acp::runtime::permissions::PermissionDecision::Ask {
            let None = crate::acp::runtime::later_permission::resolve_later_permission(
                request_id,
                crate::acp::runtime::permissions::PermissionDecision::Ask,
                None,
            )
            .map_err(|_| AdapterPumpError::Intent)?
            else {
                return Err(AdapterPumpError::Intent);
            };
            return Ok(());
        }
        let egress = self.adapter.command(AdapterCommand::ResolvePermission {
            request_id: SageServerRequestId::parse(pending.intent.external_request_id.as_str())
                .map_err(|_| AdapterPumpError::Intent)?,
            decision,
        })?;
        let intent = pending.intent.clone();
        if egress.token().is_mandatory()
            && self
                .adapter
                .commit_mandatory_egress(egress.token())
                .is_err()
        {
            self.teardown = true;
            return Err(AdapterPumpError::TeardownRequired);
        }
        let committed = match decision {
            crate::acp::runtime::permissions::PermissionDecision::AllowOnce
            | crate::acp::runtime::permissions::PermissionDecision::AllowAlways => {
                ServerRequestState::AllowCommitted
            }
            crate::acp::runtime::permissions::PermissionDecision::Deny => {
                ServerRequestState::DeclineCommitted
            }
            crate::acp::runtime::permissions::PermissionDecision::Ask => unreachable!(),
        };
        let revision = match self
            .journal
            .transition_permission(&intent, committed, now_millis)
            .await
        {
            Ok(revision) => revision,
            Err(error @ PumpJournalError::CommitUnknown) => {
                self.teardown = true;
                return Err(AdapterPumpError::Journal(error));
            }
            Err(PumpJournalError::Rejected) => {
                self.teardown = true;
                return Err(AdapterPumpError::TeardownRequired);
            }
        };
        self.publish_revision(revision)?;
        self.enqueue(egress)?;
        let revision = match self
            .journal
            .transition_permission(&intent, ServerRequestState::ResponseQueued, now_millis)
            .await
        {
            Ok(revision) => revision,
            Err(error @ PumpJournalError::CommitUnknown) => {
                self.teardown = true;
                return Err(AdapterPumpError::Journal(error));
            }
            Err(PumpJournalError::Rejected) => {
                self.teardown = true;
                return Err(AdapterPumpError::TeardownRequired);
            }
        };
        let pending = self
            .pending_permissions
            .iter_mut()
            .find(|pending| &pending.intent.request_id == request_id)
            .ok_or(AdapterPumpError::TeardownRequired)?;
        pending.decision = Some(decision);
        self.publish_revision(revision)
    }

    pub(crate) fn queue_prompt(
        &mut self,
        prompt: crate::acp::intent::Prompt,
        client_message_id: &RequiredText<512>,
        access_mode: crate::settings_store::SessionAccessMode,
    ) -> Result<QueueDisposition, AdapterPumpError> {
        self.ensure_live()?;
        let active = &self.intent.active_turn;
        if client_message_id != &active.client_message_id || self.terminal.is_some() {
            return Err(AdapterPumpError::NoActiveTurn);
        }
        let egress = self.adapter.command(AdapterCommand::Prompt {
            prompt,
            client_message_id: client_message_id.clone(),
            access_mode,
        })?;
        self.enqueue(egress)
    }

    pub(crate) fn queue_steer(
        &mut self,
        prompt: crate::acp::intent::Prompt,
        client_message_id: RequiredText<512>,
    ) -> Result<(QueueDisposition, u64), AdapterPumpError> {
        self.ensure_live()?;
        if self.terminal.is_some() {
            return Err(AdapterPumpError::NoActiveTurn);
        }
        let egress = self
            .adapter
            .command(AdapterCommand::Steer {
                prompt,
                client_message_id,
            })
            .map_err(|error| match error {
                AdapterRuntimeError::Operation | AdapterRuntimeError::UnsupportedCommand => {
                    AdapterPumpError::Intent
                }
                other => AdapterPumpError::Adapter(other),
            })?;
        let request_id = egress
            .token()
            .request_id()
            .ok_or(AdapterPumpError::Intent)?;
        let disposition = self.enqueue(egress)?;
        self.pending_steer_request = Some(request_id);
        self.steer_response = None;
        Ok((disposition, request_id))
    }

    pub(crate) fn take_steer_response(&mut self, request_id: u64) -> Option<bool> {
        if self.pending_steer_request != Some(request_id) {
            return None;
        }
        let response = self.steer_response.take()?;
        self.pending_steer_request = None;
        Some(response)
    }

    pub(crate) fn expire_steer(&mut self, request_id: u64) {
        if self.pending_steer_request == Some(request_id) {
            self.pending_steer_request = None;
            self.steer_response = None;
        }
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
            .and_then(|(request, replay)| (!replay).then_some(request.to_owned()))
            .or_else(|| egress.token().allow_request().map(str::to_owned));
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
            && let Some(pending) = self
                .pending_permissions
                .iter_mut()
                .find(|pending| pending.intent.external_request_id.as_str() == request)
            && matches!(
                pending.decision,
                Some(
                    crate::acp::runtime::permissions::PermissionDecision::AllowOnce
                        | crate::acp::runtime::permissions::PermissionDecision::AllowAlways
                        | crate::acp::runtime::permissions::PermissionDecision::Deny
                )
            )
            && pending.intent.external_request_id.as_str() == request
            && !pending.written
        {
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
        let Some(index) = self
            .pending_permissions
            .iter()
            .position(|pending| pending.written)
        else {
            return Ok(false);
        };
        let pending = &self.pending_permissions[index];
        if !pending.written {
            return Ok(false);
        }
        let intent = pending.intent.clone();
        let decision = pending.decision.ok_or(AdapterPumpError::TeardownRequired)?;
        let revision = match self
            .journal
            .transition_permission(
                &intent,
                match decision {
                    crate::acp::runtime::permissions::PermissionDecision::AllowOnce
                    | crate::acp::runtime::permissions::PermissionDecision::AllowAlways => {
                        ServerRequestState::ResolvedAllowed
                    }
                    crate::acp::runtime::permissions::PermissionDecision::Deny => {
                        ServerRequestState::ResolvedDeclined
                    }
                    crate::acp::runtime::permissions::PermissionDecision::Ask => {
                        return Err(AdapterPumpError::TeardownRequired);
                    }
                },
                now_millis,
            )
            .await
        {
            Ok(revision) => revision,
            Err(error @ PumpJournalError::CommitUnknown) => {
                self.teardown = true;
                return Err(AdapterPumpError::Journal(error));
            }
            Err(PumpJournalError::Rejected) => {
                self.teardown = true;
                return Err(AdapterPumpError::TeardownRequired);
            }
        };
        self.pending_permissions.swap_remove(index);
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
