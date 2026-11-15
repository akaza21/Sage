use super::*;

#[allow(dead_code, reason = "P-02 sealed egress encoder is adapter-deferred")]
impl ClaudeRuntime {
    /// It shares Claude's concrete frame decoder, including duplicate-key/depth/node limits, but
    /// is not exposed by the installed runtime while Decision B remains in force.
    pub(crate) fn ingest_deferred_read_text_file(
        &mut self,
        frame: &Frame,
        expected: &crate::acp::runtime::read_text_file::ExpectedOwnerScope,
        owners: &impl crate::acp::runtime::read_text_file::DeferredReadTextOwnerResolver,
        service: &crate::acp::read_text_file::ReadTextFileService,
    ) -> Result<crate::acp::runtime::read_text_file::DeferredReadTextFile, AdapterRuntimeError>
    {
        let value = decode_frame(frame.as_bytes()).map_err(|_| AdapterRuntimeError::Frame)?;
        if value.get("method").and_then(Value::as_str)
            != Some(crate::acp::runtime::read_text_file::ACP_READ_TEXT_FILE_METHOD)
            || !expected.matches_runtime(&self.binding_digest, &self.session_id, self.runtime_epoch)
        {
            return Err(AdapterRuntimeError::Frame);
        }
        let session = self.active.as_ref().ok_or(AdapterRuntimeError::Frame)?;
        crate::acp::runtime::read_text_file::route_deferred_read_text_file(
            value,
            session.external_id.as_str(),
            expected,
            owners,
            service,
        )
        .map_err(|_| AdapterRuntimeError::Frame)
    }
}
