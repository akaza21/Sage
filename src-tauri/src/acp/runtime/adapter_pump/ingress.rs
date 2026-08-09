use super::{
    coalescer::DeferredIngressState,
    normalize::{control_critical, validate_batch, validate_deny_pairs},
    *,
};

#[derive(Clone, Copy)]
enum IngestMode {
    Immediate,
    Deferred,
}

impl<A: HarnessAdapter, J: AdapterBatchJournal, R: ResourceMapper> AdapterPump<A, J, R> {
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

    pub(crate) async fn drain_stdout(&mut self, now_millis: i64) -> Result<bool, AdapterPumpError> {
        let frame = match self.decode_pending() {
            Ok(frame) => frame,
            Err(AdapterPumpError::Frame) => return self.latch_frame(),
            Err(error) => return Err(error),
        };
        self.accept_optional(frame, now_millis, IngestMode::Immediate)
            .await
    }

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

    pub(crate) async fn flush_deferred_if_needed(
        &mut self,
        now_millis: i64,
    ) -> Result<bool, AdapterPumpError> {
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
        let was_aligned = self.decoder_frame_aligned;
        let frame = self.decoder.push_chunk(chunk).map_err(|_| {
            self.decoder_frame_aligned = false;
            AdapterPumpError::Frame
        })?;
        self.decoder_frame_aligned = match &frame {
            Some(_) => !self.decoder.has_pending_input(),
            None => was_aligned && chunk.is_empty(),
        };
        Ok(frame)
    }

    fn decode_pending(&mut self) -> Result<Option<Frame>, AdapterPumpError> {
        self.ensure_live()?;
        let frame = self.decoder.drain_pending().map_err(|_| {
            self.decoder_frame_aligned = false;
            AdapterPumpError::Frame
        })?;
        if frame.is_some() && !self.decoder.has_pending_input() {
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
        self.decoder_frame_aligned = !self.decoder.has_pending_input();
        Ok(frame)
    }

    async fn accept_optional(
        &mut self,
        frame: Option<Frame>,
        now_millis: i64,
        mode: IngestMode,
    ) -> Result<bool, AdapterPumpError> {
        let Some(frame) = frame else { return Ok(false) };
        self.ensure_live()?;
        if validate_structure(&frame).is_err() {
            return match mode {
                IngestMode::Immediate => self.latch_frame(),
                IngestMode::Deferred => self.fail_deferred_frame().await,
            };
        }
        let batch = match self.adapter.ingest(&frame, &self.resources) {
            Ok(batch) => batch,
            Err(AdapterRuntimeError::MandatoryDenyTeardown) => {
                return self.fail_deferred_mandatory_deny(mode).await;
            }
            Err(AdapterRuntimeError::Frame) => {
                return match mode {
                    IngestMode::Immediate => self.latch_frame(),
                    IngestMode::Deferred => self.fail_deferred_frame().await,
                };
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
            let fact = match self.normalize(event, now_millis, prospective_terminal) {
                Ok(value) => value,
                Err(error) => return self.fail_deferred_protocol(mode, error).await,
            };
            if let Some(fact) = fact {
                #[cfg(feature = "acp-fixture")]
                if matches!(
                    &fact.event,
                    DurableAdapterEvent::Item { .. } | DurableAdapterEvent::ActivityItem(_)
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

    async fn fail_deferred_frame<T>(&mut self) -> Result<T, AdapterPumpError> {
        if !self.pending.is_empty() && self.flush_deferred().await.is_err() {
            self.pending.clear();
            self.teardown = true;
            return Err(AdapterPumpError::TeardownRequired);
        }
        self.pending.clear();
        self.teardown = true;
        Err(AdapterPumpError::Frame)
    }

    async fn fail_deferred_mandatory_deny<T>(
        &mut self,
        mode: IngestMode,
    ) -> Result<T, AdapterPumpError> {
        if matches!(mode, IngestMode::Deferred) && !self.pending.is_empty() {
            let _ = self.flush_deferred().await;
        }
        self.pending.clear();
        self.teardown = true;
        Err(AdapterPumpError::Adapter(
            AdapterRuntimeError::MandatoryDenyTeardown,
        ))
    }

    async fn fail_deferred_protocol<T>(
        &mut self,
        mode: IngestMode,
        error: AdapterPumpError,
    ) -> Result<T, AdapterPumpError> {
        if matches!(mode, IngestMode::Deferred)
            && !self.pending.is_empty()
            && self.flush_deferred().await.is_err()
        {
            self.pending.clear();
            self.teardown = true;
            return Err(AdapterPumpError::TeardownRequired);
        }
        self.pending.clear();
        self.teardown = true;
        Err(error)
    }

    fn fail_capacity<T>(&mut self, error: AdapterPumpError) -> Result<T, AdapterPumpError> {
        self.pending.clear();
        self.teardown = true;
        Err(error)
    }
}
