use super::*;

impl OmpRuntime {
    /// Deferred-only P-02 ingress belongs at this concrete runtime boundary,
    /// where Sage's binding/session/epoch and active external session are all
    /// immutable or established. It is intentionally not a live pump.
    pub(crate) fn ingest_deferred_read_text_file(
        &mut self,
        raw: &[u8],
        expected: &crate::acp::runtime::read_text_file::ExpectedOwnerScope,
        owners: &impl crate::acp::runtime::read_text_file::DeferredReadTextOwnerResolver,
        service: &crate::acp::service::ReadTextFileService,
    ) -> Result<crate::acp::runtime::read_text_file::DeferredReadTextFile, AdapterRuntimeError>
    {
        let value = crate::acp::bindings::omp::wire::decode_frame(raw)
            .map_err(|_| AdapterRuntimeError::Frame)?;
        if value.get("method").and_then(serde_json::Value::as_str)
            != Some(crate::acp::runtime::read_text_file::ACP_READ_TEXT_FILE_METHOD)
            || !expected.matches_runtime(&self.binding_digest, &self.session_id, self.runtime_epoch)
        {
            return Err(AdapterRuntimeError::Frame);
        }
        let external_session = self
            .external_session
            .as_ref()
            .ok_or(AdapterRuntimeError::Frame)?;
        crate::acp::runtime::read_text_file::route_deferred_read_text_file(
            value,
            external_session.as_str(),
            expected,
            owners,
            service,
        )
        .map_err(|_| AdapterRuntimeError::Frame)
    }
}
