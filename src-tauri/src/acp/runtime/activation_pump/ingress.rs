use super::{capabilities, *};
use crate::acp::transport::Frame;

pub(crate) fn is_available_commands_update(frame: &Frame, expected_session_id: &str) -> bool {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(frame.as_bytes()) else {
        return false;
    };
    let Some(params) = value.get("params") else {
        return false;
    };
    value.get("jsonrpc").and_then(serde_json::Value::as_str) == Some("2.0")
        && value.get("method").and_then(serde_json::Value::as_str) == Some("session/update")
        && value.get("id").is_none()
        && value.get("result").is_none()
        && value.get("error").is_none()
        && params.get("sessionId").and_then(serde_json::Value::as_str) == Some(expected_session_id)
        && params
            .get("update")
            .and_then(serde_json::Value::as_object)
            .is_some_and(|update| {
                update
                    .get("sessionUpdate")
                    .and_then(serde_json::Value::as_str)
                    == Some("available_commands_update")
                    && update
                        .get("availableCommands")
                        .is_some_and(serde_json::Value::is_array)
            })
}

impl<A: HarnessAdapter, J: ActivationJournal, R: ResourceMapper> ActivationPump<A, J, R> {
    pub(crate) fn flush_one<T: ActivationTransport>(
        &mut self,
        transport: &mut T,
    ) -> Result<ActivationFlush, ActivationPumpError> {
        self.ensure_live()?;
        let Some(egress) = self.egress.pop() else {
            return Ok(ActivationFlush::Idle);
        };
        let outcome = transport.write(egress.frame());
        let token = egress.into_token();
        let resolution = EgressResolution::from(outcome);
        let disposition = match outcome {
            FrameWriteOutcome::Complete => ActivationFlush::Written,
            FrameWriteOutcome::ZeroByteFailure => ActivationFlush::RetriedZeroBytes,
            FrameWriteOutcome::DeliveryUnknown { .. } => ActivationFlush::DeliveryUnknown,
        };
        if self.adapter.resolve_egress(token, resolution).is_err() {
            self.fail_closed();
            return Err(ActivationPumpError::TeardownRequired);
        }
        match outcome {
            FrameWriteOutcome::DeliveryUnknown { .. } => {
                self.fail_closed();
                Ok(ActivationFlush::DeliveryUnknown)
            }
            FrameWriteOutcome::ZeroByteFailure => {
                self.retry_zero_bytes()?;
                Ok(disposition)
            }
            FrameWriteOutcome::Complete => {
                self.advance_after_write()?;
                Ok(disposition)
            }
        }
    }

    pub(crate) async fn push_stdout_chunk(
        &mut self,
        chunk: &[u8],
        now_millis: i64,
    ) -> Result<bool, ActivationPumpError> {
        self.ensure_live()?;
        let was_aligned = self.decoder_frame_aligned;
        let frame = self.decoder.push_chunk(chunk).map_err(|_| {
            self.decoder_frame_aligned = false;
            self.fail_closed();
            ActivationPumpError::Frame
        })?;
        self.decoder_frame_aligned = match &frame {
            Some(_) => !self.decoder.has_pending_input(),
            None => was_aligned && chunk.is_empty(),
        };
        self.accept_optional(frame, now_millis).await
    }

    pub(crate) async fn drain_stdout(
        &mut self,
        now_millis: i64,
    ) -> Result<bool, ActivationPumpError> {
        self.ensure_live()?;
        let frame = self.decoder.drain_pending().map_err(|_| {
            self.decoder_frame_aligned = false;
            self.fail_closed();
            ActivationPumpError::Frame
        })?;
        if frame.is_some() && !self.decoder.has_pending_input() {
            self.decoder_frame_aligned = true;
        }
        self.accept_optional(frame, now_millis).await
    }

    pub(crate) async fn finish_stdout(
        &mut self,
        now_millis: i64,
    ) -> Result<bool, ActivationPumpError> {
        self.ensure_live()?;
        let frame = self.decoder.finish().map_err(|_| {
            self.decoder_frame_aligned = false;
            self.fail_closed();
            ActivationPumpError::Frame
        })?;
        self.decoder_frame_aligned = !self.decoder.has_pending_input();
        self.accept_optional(frame, now_millis).await
    }

    async fn accept_optional(
        &mut self,
        frame: Option<Frame>,
        now_millis: i64,
    ) -> Result<bool, ActivationPumpError> {
        let Some(frame) = frame else { return Ok(false) };
        let batch = if validate_structure(&frame).is_err() {
            self.adapter.discard_invalid_response(&frame).map_err(|_| {
                self.fail_closed();
                ActivationPumpError::Frame
            })?
        } else {
            if self.is_ready_handoff_available_commands_update(&frame) {
                return Ok(true);
            }
            self.adapter
                .ingest(&frame, &self.resources)
                .map_err(|error| {
                    self.fail_closed();
                    ActivationPumpError::Adapter(error)
                })?
        };
        match self.accept_batch(batch, now_millis).await {
            Ok(()) => Ok(true),
            Err(error) => {
                if !matches!(
                    error,
                    ActivationPumpError::BootstrapRequired | ActivationPumpError::RecoveryRequired
                ) && !self.teardown_required()
                {
                    self.fail_closed();
                }
                Err(error)
            }
        }
    }

    fn is_ready_handoff_available_commands_update(&self, frame: &Frame) -> bool {
        self.phase == Phase::Ready
            && self.ready.as_ref().is_some_and(|ready| {
                super::is_available_commands_update(frame, ready.external_session_id.as_str())
            })
    }

    async fn accept_batch(
        &mut self,
        batch: crate::acp::bindings::adapter::AdapterBatch,
        now_millis: i64,
    ) -> Result<(), ActivationPumpError> {
        if batch.actual_bytes() > LIMITS.queues.journal_bytes_per_transaction as usize {
            self.fail_closed();
            return Err(ActivationPumpError::Capacity);
        }
        let (events, egress) = batch.into_parts();
        if events.len() > LIMITS.queues.journal_facts_per_transaction as usize {
            self.fail_closed();
            return Err(ActivationPumpError::Capacity);
        }
        let mut ready = None;
        for event in events {
            match event {
                AdapterEvent::Initialized { capabilities } => {
                    if self.phase != Phase::AwaitingInitialized || self.capabilities.is_some() {
                        return self.protocol_error();
                    }
                    let adapter_capabilities = self
                        .adapter
                        .capabilities()
                        .ok_or(ActivationPumpError::Intent)?;
                    if adapter_capabilities != capabilities {
                        return self.protocol_error();
                    }
                    self.capabilities = Some(capabilities::intersect(
                        &self.intent.curated_ceiling,
                        &capabilities,
                        &self.intent.sage_policy,
                    )?);
                    self.phase = Phase::Initialized;
                }
                AdapterEvent::Models {
                    models,
                    next_cursor,
                } => self.accept_models(models, next_cursor)?,
                AdapterEvent::SessionReady { id, loaded } => {
                    if self.phase != Phase::AwaitingSession || ready.is_some() || !egress.is_empty()
                    {
                        return self.protocol_error();
                    }
                    ready = Some(self.validate_session_ready(id, loaded)?);
                }
                AdapterEvent::IgnoredLate
                | AdapterEvent::IgnoredDuplicate
                | AdapterEvent::Absorbed => {}
                AdapterEvent::Error(_)
                | AdapterEvent::Unsupported(_)
                | AdapterEvent::Acknowledged
                | AdapterEvent::SteerResponse { .. }
                | AdapterEvent::Turn { .. }
                | AdapterEvent::TerminalReason { .. }
                | AdapterEvent::Item { .. }
                | AdapterEvent::ChildThreadItem { .. }
                | AdapterEvent::ChildLiveItem { .. }
                | AdapterEvent::ChildTurnStatus { .. }
                | AdapterEvent::FilePair { .. }
                | AdapterEvent::ActivityItem(_)
                | AdapterEvent::Usage { .. }
                | AdapterEvent::PermissionRequest { .. }
                | AdapterEvent::PermissionResolved { .. } => return self.protocol_error(),
            }
        }
        for value in egress {
            if self.phase == Phase::AwaitingInitialized {
                self.enqueue_initialization(value)?;
            } else if self.phase == Phase::Initialized && value.token().is_notification() {
                self.enqueue(value)?;
            } else {
                return self.protocol_error();
            }
        }
        if let Some(value) = ready {
            self.commit_ready(value, now_millis).await?;
        } else if self.phase == Phase::Initialized && self.queues_empty() {
            self.queue_session()?;
        }
        Ok(())
    }

    pub(super) fn enqueue_initialization(
        &mut self,
        value: AdapterEgress,
    ) -> Result<(), ActivationPumpError> {
        if value.token().request_id().is_none() || value.token().is_control() {
            return self.protocol_error();
        }
        if self.initialization_requests >= LIMITS.queues.outstanding_rpc_per_session {
            let _ = self
                .adapter
                .resolve_egress(value.into_token(), EgressResolution::QueueRejected);
            return self.capacity_error();
        }
        self.initialization_requests += 1;
        self.enqueue(value)
    }

    fn accept_models(
        &mut self,
        values: Vec<AdapterModel>,
        next_cursor: Option<PageCursor>,
    ) -> Result<(), ActivationPumpError> {
        if !matches!(self.phase, Phase::Initialized | Phase::AwaitingSession) {
            return self.protocol_error();
        }
        let count = self
            .models
            .len()
            .checked_add(values.len())
            .ok_or(ActivationPumpError::Capacity)?;
        if count > LIMITS.wire.models_or_config_entries as usize {
            return self.capacity_error();
        }
        let mut candidate = Vec::new();
        candidate
            .try_reserve_exact(count)
            .map_err(|_| ActivationPumpError::Capacity)?;
        candidate.extend(self.models.iter().cloned());
        candidate.extend(values);
        if crate::acp::semantic_ingress::validate_models(&candidate).is_err() {
            return self.capacity_error();
        }
        self.models = candidate;
        self.next_model_cursor = next_cursor;
        Ok(())
    }

    fn validate_session_ready(
        &mut self,
        id: crate::acp::bindings::adapter::AdapterIdentity,
        loaded: bool,
    ) -> Result<ActivatedSession, ActivationPumpError> {
        let mode_matches = match &self.intent.mode {
            ActivationMode::New { .. } => !loaded,
            ActivationMode::Resume {
                external_session_id,
                ..
            } => !loaded && &id.external_id == external_session_id,
            ActivationMode::Load {
                external_session_id,
            } => loaded && &id.external_id == external_session_id,
        };
        if !mode_matches
            || id.binding_digest != self.intent.binding_digest
            || id.runtime_epoch != self.intent.runtime_epoch
            || id.session_id != self.intent.session_id
            || id.turn_id.is_some()
            || id.turn_client_message_id.is_some()
            || self.capabilities.is_none()
        {
            return self.protocol_error();
        }
        Ok(ActivatedSession {
            binding_digest: self.intent.binding_digest,
            project_id: self.intent.project_id.clone(),
            chat_id: self.intent.chat_id.clone(),
            session_id: self.intent.session_id.clone(),
            external_session_id: id.external_id,
            runtime_epoch: self.intent.runtime_epoch,
            capabilities: self.capabilities.take().expect("capabilities checked"),
            models: std::mem::take(&mut self.models),
            next_model_cursor: self.next_model_cursor.take(),
        })
    }

    async fn commit_ready(
        &mut self,
        ready: ActivatedSession,
        now_millis: i64,
    ) -> Result<(), ActivationPumpError> {
        let capabilities_json = serde_json::to_string(&ready.capabilities)
            .map_err(|_| ActivationPumpError::Capacity)?;
        let command = CompleteActivationCommand {
            model_context: self.activation_model_context(&ready),
            project_id: ready.project_id.clone(),
            chat_id: ready.chat_id.clone(),
            session_id: ready.session_id.clone(),
            runtime_epoch: ready.runtime_epoch,
            binding_digest: digest(&ready.binding_digest),
            artifact_digest: self.intent.artifact_digest.clone(),
            external_session_id: ready.external_session_id.clone(),
            capabilities_json,
            now_millis,
        };
        match self.journal.commit_ready(command).await {
            Ok(receipt) if receipt.projection_revision != ProjectionRevision::ZERO => {
                self.ready = Some(ready);
                self.phase = Phase::Ready;
                Ok(())
            }
            Err(ActivationJournalError::Rejected) => {
                self.fail_closed();
                Err(ActivationPumpError::TeardownRequired)
            }
            Ok(_) | Err(ActivationJournalError::CommitUnknown) => {
                self.phase = Phase::RecoveryRequired;
                Err(ActivationPumpError::RecoveryRequired)
            }
            Err(ActivationJournalError::BootstrapRequired) => {
                self.phase = Phase::RecoveryRequired;
                Err(ActivationPumpError::BootstrapRequired)
            }
            Err(ActivationJournalError::RecoveryRequired) => {
                self.phase = Phase::RecoveryRequired;
                Err(ActivationPumpError::RecoveryRequired)
            }
        }
    }

    fn retry_zero_bytes(&mut self) -> Result<(), ActivationPumpError> {
        match self.phase {
            Phase::InitializeQueued if self.initialize_attempts < 2 => self.queue_initialize(),
            Phase::SessionQueued if self.session_attempts < 2 => self.queue_session(),
            _ => {
                self.fail_closed();
                Err(ActivationPumpError::RetryExhausted)
            }
        }
    }

    fn advance_after_write(&mut self) -> Result<(), ActivationPumpError> {
        match self.phase {
            Phase::InitializeQueued => self.phase = Phase::AwaitingInitialized,
            Phase::SessionQueued => self.phase = Phase::AwaitingSession,
            Phase::Initialized if self.queues_empty() => self.queue_session()?,
            _ => {}
        }
        Ok(())
    }

    fn queues_empty(&self) -> bool {
        self.egress.data_usage().0 == 0 && self.egress.control_usage().0 == 0
    }

    fn protocol_error<T>(&mut self) -> Result<T, ActivationPumpError> {
        self.fail_closed();
        Err(ActivationPumpError::Intent)
    }

    fn capacity_error<T>(&mut self) -> Result<T, ActivationPumpError> {
        self.fail_closed();
        Err(ActivationPumpError::Capacity)
    }
}
