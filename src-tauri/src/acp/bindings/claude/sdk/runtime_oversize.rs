use serde_json::Value;

use super::{ClaudeSdkRuntime, SdkBridge, sdk_frame};
use crate::acp::{
    bindings::{
        adapter::{
            AdapterBatch, AdapterBatchBuilder, AdapterEgress, AdapterEvent, AdapterRuntimeError,
            EgressToken, ResourceMapper,
        },
        claude::wire::decode_frame,
    },
    transport::Frame,
};

/// Paths in a frame read without its structure checks are not mapped into
/// the project; they show as unavailable.
struct NoResources;

impl ResourceMapper for NoResources {
    fn map_existing(
        &self,
        _absolute_path: &str,
    ) -> Result<
        sage_acp_protocol::intent::SafeResourceRef,
        sage_acp_protocol::intent::UnavailableResourceReason,
    > {
        Err(sage_acp_protocol::intent::UnavailableResourceReason::Foreign)
    }
}

impl ClaudeSdkRuntime {
    pub(super) fn ingest_oversize(
        &mut self,
        frame: &Frame,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        let mut sdk: Value =
            serde_json::from_slice(frame.as_bytes()).map_err(|_| AdapterRuntimeError::Frame)?;
        let object = sdk.as_object_mut().ok_or(AdapterRuntimeError::Frame)?;
        object.remove("tool_use_result");
        object.remove("tool_result_meta");
        let reduced = serde_json::to_vec(&sdk).map_err(|_| AdapterRuntimeError::Frame)?;
        if let Ok(sdk) = decode_frame(&reduced) {
            return self.ingest_sdk(sdk, &NoResources);
        }
        if let Some(essentials) = SdkBridge::essentials(&sdk)
            && let Ok(essentials) = serde_json::to_vec(&essentials)
            && let Ok(sdk) = decode_frame(&essentials)
        {
            return self.ingest_sdk(sdk, &NoResources);
        }
        let mut batch = AdapterBatchBuilder::new();
        if let Some(reply) = SdkBridge::refusal(&sdk) {
            batch.push_egress(AdapterEgress::new(
                sdk_frame(&reply)?,
                EgressToken::protocol_notification(),
            ))?;
        }
        batch.push_event(AdapterEvent::Absorbed)?;
        Ok(batch.finish())
    }
}
