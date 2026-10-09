use super::*;
use crate::acp::bindings::adapter::ResourceMapper;
use crate::acp::transport::{Frame, request_payload_digest};

impl CodexRuntime {
    pub(super) fn ingest_frame(
        &mut self,
        frame: &Frame,
        resources: &dyn ResourceMapper,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        if self.poisoned {
            return Err(AdapterRuntimeError::Operation);
        }
        if self.inner.child_read_pending() || self.inner.is_child_read_tombstone_id_active() {
            let value: serde_json::Value =
                serde_json::from_slice(frame.as_bytes()).map_err(|_| AdapterRuntimeError::Frame)?;
            if let Some(id) = value.get("id").and_then(serde_json::Value::as_u64) {
                if self.inner.is_child_read_tombstone(id)
                    && value.get("method").is_none()
                    && (value.get("result").is_some() || value.get("error").is_some())
                {
                    return one(AdapterEvent::IgnoredLate);
                }
                if let Some((pending, child, parent)) = self.inner.child_read_scope()
                    && id == pending
                {
                    let snapshot = super::super::child_transcript::normalize_read_response(
                        frame.as_bytes(),
                        id,
                        child,
                        parent,
                        resources,
                    )
                    .map_err(|_| AdapterRuntimeError::Frame)?;
                    self.inner.complete_child_read(id).map_err(map_adapter)?;
                    self.native_rpc_reply =
                        Some(crate::acp::goal::NativeRpcReply::ChildTranscript(snapshot));
                    return one(AdapterEvent::IgnoredLate);
                }
            }
        }
        let mapper = MapperBridge(resources);
        let disposition = self
            .inner
            .ingest_with_declarations(frame.as_bytes(), &mapper, &self.declared_test_commands)
            .map_err(map_adapter)?;
        self.record_browser_failure_diagnostic(&disposition, frame.as_bytes());
        let request_digest =
            request_payload_digest(frame).map_err(|_| AdapterRuntimeError::Frame)?;
        self.translate(disposition, request_digest)
    }
}
