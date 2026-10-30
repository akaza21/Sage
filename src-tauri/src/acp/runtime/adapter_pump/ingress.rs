use super::{
    coalescer::DeferredIngressState,
    normalize::{control_critical, validate_batch, validate_deny_pairs},
    *,
};
use crate::acp::runtime::terminal_detail;

mod shape;
mod teardown;
use shape::IngestMode;

impl<A: HarnessAdapter, J: AdapterBatchJournal, R: ResourceMapper> AdapterPump<A, J, R> {
    #[cfg(test)]
    pub(crate) async fn push_stdout_chunk(
        &mut self,
        chunk: &[u8],
        now_millis: i64,
    ) -> Result<bool, AdapterPumpError> {
        let frame = match self.decode_chunk(chunk) {
            Ok(frame) => frame,
            Err(AdapterPumpError::Frame) => return self.latch_frame(),
            Err(error) => return Err(error),
        };
        self.accept_optional(frame, now_millis, IngestMode::Immediate)
            .await
    }

    #[cfg(test)]
    pub(crate) async fn drain_stdout(&mut self, now_millis: i64) -> Result<bool, AdapterPumpError> {
        let frame = match self.decode_pending() {
            Ok(frame) => frame,
            Err(AdapterPumpError::Frame) => return self.latch_frame(),
            Err(error) => return Err(error),
        };
        self.accept_optional(frame, now_millis, IngestMode::Immediate)
            .await
    }

    #[cfg(test)]
    pub(crate) async fn finish_stdout(
        &mut self,
        now_millis: i64,
    ) -> Result<bool, AdapterPumpError> {
        let frame = match self.decode_finish() {
            Ok(frame) => frame,
            Err(AdapterPumpError::Frame) => return self.latch_frame(),
            Err(error) => return Err(error),
        };
        self.accept_optional(frame, now_millis, IngestMode::Immediate)
            .await
    }

    pub(crate) async fn push_stdout_chunk_deferred(
        &mut self,
        chunk: &[u8],
        now_millis: i64,
    ) -> Result<bool, AdapterPumpError> {
        let frame = match self.decode_chunk(chunk) {
            Ok(frame) => frame,
            Err(AdapterPumpError::Frame) => return self.fail_deferred_frame().await,
            Err(error) => return Err(error),
        };
        self.accept_optional(frame, now_millis, IngestMode::Deferred)
            .await
    }

    pub(crate) async fn drain_stdout_deferred(
        &mut self,
        now_millis: i64,
    ) -> Result<bool, AdapterPumpError> {
        let frame = match self.decode_pending() {
            Ok(frame) => frame,
            Err(AdapterPumpError::Frame) => return self.fail_deferred_frame().await,
            Err(error) => return Err(error),
        };
        self.accept_optional(frame, now_millis, IngestMode::Deferred)
            .await
    }

    pub(crate) async fn drain_stdout_after_config_rejection(
        &mut self,
        now_millis: i64,
    ) -> Result<bool, AdapterPumpError> {
        let frame = match self.decode_pending() {
            Ok(frame) => frame,
            Err(AdapterPumpError::Frame) => return self.fail_deferred_frame().await,
            Err(error) => return Err(error),
        };
        let Some(frame) = frame else {
            return Ok(std::mem::take(&mut self.oversize_event));
        };
        self.enforce_provisional_deadline(now_millis)?;
        if validate_structure(&frame).is_err() {
            if let Ok(batch) = self.adapter.discard_invalid_response(&frame) {
                self.accept_batch(batch, now_millis, IngestMode::Deferred)
                    .await?;
                return Ok(true);
            }
            return self.fail_deferred_frame().await;
        }
        match self.adapter.ingest(&frame, &self.resources) {
            Ok(batch) => {
                self.refresh_provisional_deadline(now_millis);
                self.accept_batch(batch, now_millis, IngestMode::Deferred)
                    .await?
            }
            Err(error) if NonfatalRefusal::from_adapter(&error).is_some() => {}
            Err(AdapterRuntimeError::MandatoryDenyTeardown) => {
                return self
                    .fail_deferred_mandatory_deny(IngestMode::Deferred)
                    .await;
            }
            Err(AdapterRuntimeError::Frame) => return self.fail_deferred_frame().await,
            Err(error) => {
                if !self.pending.is_empty() {
                    let _ = self.flush_deferred().await;
                }
                self.pending.clear();
                self.teardown = true;
                return Err(error.into());
            }
        }
        Ok(true)
    }

    pub(crate) async fn finish_stdout_deferred(
        &mut self,
        now_millis: i64,
    ) -> Result<bool, AdapterPumpError> {
        let frame = match self.decode_finish() {
            Ok(frame) => frame,
            Err(AdapterPumpError::Frame) => return self.fail_deferred_frame().await,
            Err(error) => return Err(error),
        };
        let accepted = self
            .accept_optional(frame, now_millis, IngestMode::Deferred)
            .await?;
        if !self.decoder.has_pending_input() {
            self.flush_deferred().await?;
        }
        Ok(accepted)
    }

    pub(crate) fn has_pending_stdout_input(&self) -> bool {
        self.decoder.has_pending_input()
    }

    pub(crate) async fn flush_deferred_if_needed(
        &mut self,
        now_millis: i64,
    ) -> Result<bool, AdapterPumpError> {
        self.enforce_provisional_deadline(now_millis)?;
        if matches!(
            self.pending.state(now_millis),
            DeferredIngressState::FlushNeeded { .. }
        ) {
            self.flush_deferred().await
        } else {
            Ok(false)
        }
    }

    pub(crate) async fn flush_deferred(&mut self) -> Result<bool, AdapterPumpError> {
        self.ensure_live()?;
        if self.pending.is_empty() {
            return Ok(false);
        }
        self.commit_pending(Vec::new()).await?;
        Ok(true)
    }

    fn decode_chunk(&mut self, chunk: &[u8]) -> Result<Option<Frame>, AdapterPumpError> {
        self.ensure_live()?;
        self.configure_oversize_mode();
        let was_aligned = self.decoder_frame_aligned;
        let frame = self.decoder.push_chunk(chunk).map_err(|_| {
            self.decoder_frame_aligned = false;
            AdapterPumpError::Frame
        })?;
        self.accept_oversize_event()?;
        self.decoder_frame_aligned = match &frame {
            Some(_) => !self.decoder.has_pending_input(),
            None if self.oversize_event => !self.decoder.has_pending_input(),
            None => was_aligned && chunk.is_empty(),
        };
        Ok(frame)
    }

    fn decode_pending(&mut self) -> Result<Option<Frame>, AdapterPumpError> {
        self.ensure_live()?;
        self.configure_oversize_mode();
        let frame = self.decoder.drain_pending().map_err(|_| {
            self.decoder_frame_aligned = false;
            AdapterPumpError::Frame
        })?;
        self.accept_oversize_event()?;
        if (frame.is_some() || self.oversize_event) && !self.decoder.has_pending_input() {
            self.decoder_frame_aligned = true;
        }
        Ok(frame)
    }

    fn decode_finish(&mut self) -> Result<Option<Frame>, AdapterPumpError> {
        self.ensure_live()?;
        let frame = self.decoder.finish().map_err(|_| {
            self.decoder_frame_aligned = false;
            AdapterPumpError::Frame
        })?;
        self.accept_oversize_event()?;
        self.decoder_frame_aligned = !self.decoder.has_pending_input();
        Ok(frame)
    }

    fn configure_oversize_mode(&mut self) {
        if self.adapter.child_read_pending() {
            self.decoder.enable_recoverable_oversize();
        } else {
            self.decoder.disable_recoverable_oversize();
        }
    }

    fn accept_oversize_event(&mut self) -> Result<(), AdapterPumpError> {
        if let Some(id) = self.decoder.take_oversize_id() {
            self.adapter
                .accept_oversized_response(id)
                .map_err(|_| AdapterPumpError::Frame)?;
            self.oversize_event = true;
            self.configure_oversize_mode();
        }
        Ok(())
    }

    async fn accept_optional(
        &mut self,
        frame: Option<Frame>,
        now_millis: i64,
        mode: IngestMode,
    ) -> Result<bool, AdapterPumpError> {
        let Some(frame) = frame else {
            return Ok(std::mem::take(&mut self.oversize_event));
        };
        self.ensure_live()?;
        self.enforce_provisional_deadline(now_millis)?;
        if validate_structure(&frame).is_err() {
            if self.skip_stdout_preamble(&frame) {
                return Ok(false);
            }
            if let Ok(batch) = self.adapter.discard_invalid_response(&frame) {
                self.accept_batch(batch, now_millis, mode).await?;
                return Ok(true);
            }
            return match mode {
                IngestMode::Immediate => self.latch_frame(),
                IngestMode::Deferred => self.fail_deferred_frame().await,
            };
        }
        self.stdout_preamble = None;
        let batch = match self.adapter.ingest(&frame, &self.resources) {
            Ok(batch) => {
                self.refresh_provisional_deadline(now_millis);
                batch
            }
            Err(AdapterRuntimeError::MandatoryDenyTeardown) => {
                return self.fail_deferred_mandatory_deny(mode).await;
            }
            Err(AdapterRuntimeError::Frame) => {
                return match mode {
                    IngestMode::Immediate => self.latch_frame(),
                    IngestMode::Deferred => self.fail_deferred_frame().await,
                };
            }
            Err(error) if let Some(refusal) = NonfatalRefusal::from_adapter(&error) => {
                if matches!(mode, IngestMode::Deferred) && !self.pending.is_empty() {
                    self.flush_deferred().await?;
                }
                if matches!(refusal, NonfatalRefusal::SignIn) {
                    let detail = self.adapter.refusal_detail();
                    self.terminalize_active_turn(
                        TurnTerminalReason::AuthRequired,
                        detail
                            .as_ref()
                            .and_then(|detail| terminal_detail::auth_required(detail.message())),
                        detail
                            .as_ref()
                            .and_then(|detail| terminal_detail::remediation(detail.remediation())),
                        now_millis,
                        frame.len(),
                    )
                    .await?;
                }
                return Err(refusal.into());
            }
            Err(error) => {
                if matches!(mode, IngestMode::Deferred) && !self.pending.is_empty() {
                    let _ = self.flush_deferred().await;
                }
                self.pending.clear();
                self.teardown = true;
                return Err(error.into());
            }
        };
        self.accept_batch(batch, now_millis, mode).await?;
        Ok(true)
    }

    async fn accept_batch(
        &mut self,
        batch: AdapterBatch,
        now_millis: i64,
        mode: IngestMode,
    ) -> Result<(), AdapterPumpError> {
        let source_bytes = batch.actual_bytes();
        let (events, egress) = batch.into_parts();
        if let Err(error) = validate_batch(&events, &egress, source_bytes) {
            return self.fail_deferred_protocol(mode, error).await;
        }
        let mut prospective_terminal = self.terminal;
        let mut facts = Vec::new();
        if facts.try_reserve_exact(events.len()).is_err() {
            return self
                .fail_deferred_protocol(mode, AdapterPumpError::BatchCapacity)
                .await;
        }
        for event in events {
            if let AdapterEvent::SteerResponse {
                request_id,
                accepted,
            } = &event
                && self.pending_steer_request == Some(*request_id)
            {
                self.steer_response = Some(*accepted);
            }
            let fact = match self.normalize(event, now_millis, prospective_terminal) {
                Ok(value) => value,
                Err(error) => return self.fail_deferred_protocol(mode, error).await,
            };
            if let Some(fact) = fact {
                #[cfg(feature = "acp-fixture")]
                if matches!(
                    &fact.event,
                    DurableAdapterEvent::Item { .. }
                        | DurableAdapterEvent::ChildThreadItem { .. }
                        | DurableAdapterEvent::ChildLiveItem { .. }
                        | DurableAdapterEvent::ActivityItem(_)
                ) {
                    self.journal.first_normalized_item_accepted();
                }
                if let DurableAdapterEvent::Turn { state, .. } = &fact.event
                    && state.is_terminal()
                {
                    prospective_terminal = Some(*state);
                }
                facts.push(fact);
            }
        }
        if validate_deny_pairs(&facts, &egress).is_err() {
            return self
                .fail_deferred_protocol(mode, AdapterPumpError::TeardownRequired)
                .await;
        }

        if matches!(mode, IngestMode::Immediate) {
            let mut durable = Vec::new();
            if durable.try_reserve_exact(facts.len()).is_err() {
                return self.fail_capacity(AdapterPumpError::BatchCapacity);
            }
            durable.extend(facts.into_iter().map(|fact| fact.event));
            return self.commit_and_egress(durable, source_bytes, egress).await;
        }

        let force = !egress.is_empty() || facts.iter().any(control_critical);
        if self.pending.would_exceed(&facts, source_bytes) && !self.pending.is_empty() {
            self.commit_pending(Vec::new()).await?;
        }
        if self.pending.would_exceed(&facts, source_bytes) {
            return self.fail_capacity(AdapterPumpError::BatchCapacity);
        }
        if let Err(error) = self.pending.push(facts, source_bytes, now_millis) {
            return self.fail_capacity(error);
        }
        let threshold = matches!(
            self.pending.state(now_millis),
            DeferredIngressState::FlushNeeded { .. }
        );
        if force || threshold {
            self.commit_pending(egress).await?;
        }
        Ok(())
    }

    async fn commit_pending(&mut self, egress: Vec<AdapterEgress>) -> Result<(), AdapterPumpError> {
        let (durable, actual_bytes) = self.pending.take().inspect_err(|_| {
            self.teardown = true;
        })?;
        self.commit_and_egress(durable, actual_bytes, egress).await
    }
}
