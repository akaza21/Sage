use super::*;

impl<A: HarnessAdapter, J: AdapterBatchJournal, R: ResourceMapper> AdapterPump<A, J, R> {
    pub(crate) fn next_request_deadline_millis(&self, now_millis: i64) -> Option<i64> {
        let remaining = self
            .adapter
            .next_request_deadline()?
            .saturating_duration_since(std::time::Instant::now());
        let delay = u64::try_from(remaining.as_nanos().div_ceil(1_000_000)).unwrap_or(u64::MAX);
        Some(now_millis.saturating_add(i64::try_from(delay).unwrap_or(i64::MAX)))
    }

    pub(crate) async fn poll_request_deadlines(
        &mut self,
        now_millis: i64,
    ) -> Result<bool, AdapterPumpError> {
        let Some(batch) = self.adapter.poll_request_deadlines(&self.resources)? else {
            return Ok(false);
        };
        self.accept_ingested(batch, now_millis, IngestMode::Deferred)
            .await?;
        Ok(true)
    }
}
