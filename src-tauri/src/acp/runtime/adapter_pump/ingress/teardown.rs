use super::{IngestMode, *};

impl<A: HarnessAdapter, J: AdapterBatchJournal, R: ResourceMapper> AdapterPump<A, J, R> {
    pub(super) async fn terminalize_active_turn(
        &mut self,
        reason: TurnTerminalReason,
        message: Option<String>,
        remediation: Option<String>,
        now_millis: i64,
        source_bytes: usize,
    ) -> Result<(), AdapterPumpError> {
        if self.terminal.is_some() {
            return Ok(());
        }
        self.commit_and_egress(
            vec![
                DurableAdapterEvent::Turn {
                    scope: self.scope(),
                    state: TurnState::Failed,
                    now_millis,
                },
                DurableAdapterEvent::TerminalReason {
                    scope: self.scope(),
                    reason,
                    message,
                    remediation,
                    now_millis,
                },
            ],
            source_bytes,
            Vec::new(),
        )
        .await
    }

    /// Once the harness's output has ended mid-turn, its replies are shown
    /// whole before the turn is swept, as any other ending shows them.
    pub(super) async fn accept_unfinished_replies(
        &mut self,
        now_millis: i64,
    ) -> Result<bool, AdapterPumpError> {
        if self.terminal.is_some() || self.ensure_live().is_err() {
            return Ok(false);
        }
        let Ok(events) = self.adapter.unfinished_replies() else {
            return Ok(false);
        };
        if events.is_empty() {
            return Ok(false);
        }
        let batch = AdapterBatch::new(events, Vec::new())?;
        self.accept_batch(batch, now_millis, IngestMode::Deferred)
            .await?;
        Ok(true)
    }

    pub(super) async fn fail_deferred_frame<T>(&mut self) -> Result<T, AdapterPumpError> {
        if !self.pending.is_empty() && self.flush_deferred().await.is_err() {
            self.pending.clear();
            self.teardown = true;
            return Err(AdapterPumpError::TeardownRequired);
        }
        self.pending.clear();
        self.teardown = true;
        Err(AdapterPumpError::Frame)
    }

    pub(super) async fn fail_deferred_mandatory_deny<T>(
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

    pub(super) async fn fail_deferred_protocol<T>(
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

    pub(super) fn fail_capacity<T>(
        &mut self,
        error: AdapterPumpError,
    ) -> Result<T, AdapterPumpError> {
        self.pending.clear();
        self.teardown = true;
        Err(error)
    }
}
