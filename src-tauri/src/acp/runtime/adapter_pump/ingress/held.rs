use super::{IngestMode, *};

impl<A: HarnessAdapter, J: AdapterBatchJournal, R: ResourceMapper> AdapterPump<A, J, R> {
    /// Accepts `batch`, then each batch of what the adapter held back from it.
    pub(super) async fn accept_ingested(
        &mut self,
        batch: AdapterBatch,
        now_millis: i64,
        mode: IngestMode,
    ) -> Result<(), AdapterPumpError> {
        self.accept_batch(batch, now_millis, mode).await?;
        loop {
            match self.adapter.held_batch() {
                Ok(Some(batch)) => self.accept_batch(batch, now_millis, mode).await?,
                Ok(None) => return Ok(()),
                Err(error) => return self.fail_deferred_protocol(mode, error.into()).await,
            }
        }
    }
}
