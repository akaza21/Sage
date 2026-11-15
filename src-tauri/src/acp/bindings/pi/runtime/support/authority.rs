use crate::acp::bindings::adapter::{
    AdapterEgress, AdapterEvent, AdapterRuntimeError, ResourceMapper,
};
use sage_acp_protocol::{
    contract::SafeText,
    intent::NormalizedPayload,
    lifecycle::{ItemLifecycle, ToolState},
};

use super::{
    super::{DenyIntent, DenyStage, PiRuntime},
    identity::{external, tool_start_payload},
};

impl PiRuntime {
    pub(super) fn block_authority(
        &mut self,
        external_id: String,
        tool: String,
        resources: &dyn ResourceMapper,
    ) -> Result<(Option<AdapterEvent>, Option<AdapterEgress>), AdapterRuntimeError> {
        let id = external("piItemId", external_id)?;
        let mut payload = tool_start_payload(tool, None, resources)?;
        if let NormalizedPayload::ToolCall { output_summary, .. } = &mut payload {
            *output_summary = Some(
                SafeText::new(
                    "toolOutput",
                    "Pi attempted unapproved authority; blocked and abort requested",
                )
                .map_err(|_| AdapterRuntimeError::Frame)?,
            );
        }
        self.deny = Some(DenyIntent {
            request_id: id.clone(),
            command_id: String::new(),
            stage: DenyStage::Prepared,
            releases: false,
        });
        let event = self.item(
            id,
            ItemLifecycle::Cancelled,
            Some(ToolState::Declined),
            payload,
        )?;
        Ok((Some(event), self.mandatory_abort()?))
    }
}
