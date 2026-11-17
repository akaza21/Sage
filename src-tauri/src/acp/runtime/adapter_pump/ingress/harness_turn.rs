use super::{IngestMode, *};

impl<A: HarnessAdapter, J: AdapterBatchJournal, R: ResourceMapper> AdapterPump<A, J, R> {
    /// After this Turn's end, the harness's next Turn follows it in this
    /// exact session. Before it, this Turn's prompt has not reached the
    /// harness yet, and the Turn keeps the session: the run is declined.
    pub(super) fn accept_turn_started(
        &mut self,
        id: &crate::acp::bindings::adapter::AdapterIdentity,
        terminal: Option<TurnState>,
    ) -> Result<(), AdapterPumpError> {
        if !self.exact_session(id) {
            return Err(AdapterPumpError::Intent);
        }
        if terminal.is_some() {
            self.next_turn_started = true;
            return Ok(());
        }
        for egress in self.adapter.decline_turn()? {
            self.enqueue(egress)?;
        }
        Ok(())
    }

    /// Accepts the end of a Turn that only waited on the answers just
    /// written. Returns whether the harness reported anything.
    pub(crate) async fn accept_delivered_answers(
        &mut self,
        now_millis: i64,
    ) -> Result<bool, AdapterPumpError> {
        self.ensure_live()?;
        let batch = match self.adapter.answers_delivered(&self.resources) {
            Ok(Some(batch)) => batch,
            Ok(None) => return Ok(false),
            Err(error) => {
                self.teardown = true;
                return Err(error.into());
            }
        };
        self.accept_batch(batch, now_millis, IngestMode::Deferred)
            .await?;
        Ok(true)
    }

    /// Reads what the harness reported for the Turn it started on its own,
    /// now that the Turn is open.
    pub(crate) async fn adopt_started_turn(
        &mut self,
        now_millis: i64,
    ) -> Result<(), AdapterPumpError> {
        self.ensure_live()?;
        let client_message_id = self.intent.active_turn.client_message_id.clone();
        let batch = match self.adapter.adopt_turn(client_message_id, &self.resources) {
            Ok(batch) => batch,
            Err(error) => {
                self.teardown = true;
                return Err(error.into());
            }
        };
        self.accept_batch(batch, now_millis, IngestMode::Deferred)
            .await
    }
}
