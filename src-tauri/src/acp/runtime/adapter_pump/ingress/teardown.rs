use super::{IngestMode, *};
use crate::acp::runtime::terminal_detail;

impl<A: HarnessAdapter, J: AdapterBatchJournal, R: ResourceMapper> AdapterPump<A, J, R> {
    /// Fails the active turn the harness answered with a sign-in refusal.
    pub(super) async fn fail_turn_for_sign_in(
        &mut self,
        now_millis: i64,
        source_bytes: usize,
    ) -> Result<(), AdapterPumpError> {
        let (message, remediation) = self.sign_in_detail();
        self.terminalize_active_turn(
            TurnTerminalReason::AuthRequired,
            message,
            remediation,
            now_millis,
            source_bytes,
        )
        .await
    }

    /// Fails the active turn whose prompt the harness refused for sign-in
    /// before Sage wrote it. No harness frame carries that refusal, so the
    /// commit is measured by the detail it records.
    pub(crate) async fn fail_unwritten_prompt_for_sign_in(
        &mut self,
        now_millis: i64,
    ) -> Result<(), AdapterPumpError> {
        let (message, remediation) = self.sign_in_detail();
        let measured = [message, remediation]
            .into_iter()
            .flatten()
            .map(|text| text.len())
            .sum::<usize>();
        self.fail_turn_for_sign_in(now_millis, measured.max(1))
            .await
    }

    /// The harness's own words for a sign-in refusal, when it gave any.
    fn sign_in_detail(&self) -> (Option<String>, Option<String>) {
        let Some(detail) = self.adapter.refusal_detail() else {
            return (None, None);
        };
        (
            terminal_detail::auth_required(detail.message()),
            terminal_detail::remediation(detail.remediation()),
        )
    }

    async fn terminalize_active_turn(
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
                    native_turn_id: None,
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

    /// The frame left in the harness's output when it ended. With none left
    /// mid-turn, its replies are shown whole before the turn is swept, as any
    /// other ending shows them.
    pub(super) async fn accept_last_frame(
        &mut self,
        frame: Option<Frame>,
        now_millis: i64,
    ) -> Result<bool, AdapterPumpError> {
        let ended = frame.is_none();
        let accepted = self
            .accept_optional(frame, now_millis, IngestMode::Deferred)
            .await?;
        if !ended || accepted {
            return Ok(accepted);
        }
        self.accept_unfinished_replies(now_millis).await
    }

    /// Commits each reply of the active turn still streaming, shown whole,
    /// before Sage stops the harness mid-turn: the last word streaming held
    /// back would otherwise never be shown.
    pub(crate) async fn commit_unfinished_replies(
        &mut self,
        now_millis: i64,
    ) -> Result<(), AdapterPumpError> {
        if self.accept_unfinished_replies(now_millis).await? {
            self.flush_deferred().await?;
        }
        Ok(())
    }

    async fn accept_unfinished_replies(
        &mut self,
        now_millis: i64,
    ) -> Result<bool, AdapterPumpError> {
        if self.terminal.is_some() || self.ensure_live().is_err() {
            return Ok(false);
        }
        let Ok(events) = self.adapter.unfinished_replies() else {
            return Ok(false);
        };
        let mut replies = HeldOutput::of(events);
        let mut accepted = false;
        while let Some(batch) = replies.next_batch()? {
            self.accept_batch(batch, now_millis, IngestMode::Deferred)
                .await?;
            accepted = true;
        }
        Ok(accepted)
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
