use crate::acp::bindings::adapter::{AdapterEvent, AdapterRuntimeError, ResourceMapper};
use sage_acp_protocol::lifecycle::{ItemLifecycle, ToolState};

use super::super::PiRuntime;
use super::identity::{self, external};
use crate::acp::bindings::pi::ToolOutcome;

impl PiRuntime {
    pub(super) fn tool_started(
        &mut self,
        external_id: String,
        name: String,
        path: Option<String>,
        resources: &dyn ResourceMapper,
    ) -> Result<AdapterEvent, AdapterRuntimeError> {
        let id = external("piItemId", external_id.clone())?;
        let payload = identity::tool_start_payload(name, path.as_deref(), resources)?;
        self.cache_tool(external_id, payload.clone(), false)?;
        self.item(
            id,
            ItemLifecycle::Started,
            Some(ToolState::InProgress),
            payload,
        )
    }

    pub(super) fn tool_updated(
        &mut self,
        external_id: String,
        output: Option<String>,
    ) -> Result<AdapterEvent, AdapterRuntimeError> {
        let id = external("piItemId", external_id.clone())?;
        let payload = self.update_tool(&external_id, output, false)?;
        self.item(
            id,
            ItemLifecycle::Streaming,
            Some(ToolState::InProgress),
            payload,
        )
    }

    pub(super) fn tool_terminal(
        &mut self,
        external_id: String,
        outcome: ToolOutcome,
        output: Option<String>,
    ) -> Result<AdapterEvent, AdapterRuntimeError> {
        let id = external("piItemId", external_id.clone())?;
        // A finished edit shows its diff; Pi's own success line adds nothing.
        let output = output
            .filter(|_| outcome != ToolOutcome::Completed || !self.is_edit_tool(&external_id));
        let payload = self.update_tool(&external_id, output, true)?;
        let (lifecycle, state) = match outcome {
            ToolOutcome::Completed => (ItemLifecycle::Completed, ToolState::Completed),
            ToolOutcome::Failed => (ItemLifecycle::Failed, ToolState::Failed),
            ToolOutcome::Declined => (ItemLifecycle::Cancelled, ToolState::Declined),
            ToolOutcome::Cancelled => (ItemLifecycle::Cancelled, ToolState::Cancelled),
        };
        self.item(id, lifecycle, Some(state), payload)
    }
}
