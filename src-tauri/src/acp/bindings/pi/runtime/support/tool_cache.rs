use crate::acp::bindings::adapter::AdapterRuntimeError;
use sage_acp_protocol::intent::NormalizedPayload;
use sage_core::limits::LIMITS;

use super::super::PiRuntime;

impl PiRuntime {
    pub(super) fn cache_tool(
        &mut self,
        external_id: String,
        payload: NormalizedPayload,
        terminal: bool,
    ) -> Result<(), AdapterRuntimeError> {
        let turn = self
            .active_turn
            .as_mut()
            .ok_or(AdapterRuntimeError::Operation)?;
        let payload_bytes = |payload: &NormalizedPayload| {
            serde_json::to_vec(payload)
                .map(|bytes| bytes.len())
                .map_err(|_| AdapterRuntimeError::Frame)
        };
        let previous = turn
            .tools
            .get(&external_id)
            .map(payload_bytes)
            .transpose()?
            .unwrap_or(0);
        let next = if terminal {
            0
        } else {
            payload_bytes(&payload)?
        };
        let Some(bytes) = turn
            .tool_snapshot_bytes
            .checked_sub(previous)
            .and_then(|bytes| bytes.checked_add(next))
            .filter(|bytes| *bytes <= LIMITS.wire.item_bytes_per_turn as usize)
        else {
            turn.tools.clear();
            turn.tool_snapshot_bytes = 0;
            self.poisoned = true;
            return Err(AdapterRuntimeError::BatchBytes);
        };
        if terminal {
            turn.tools.remove(&external_id);
        } else {
            turn.tools.insert(external_id, payload);
        }
        turn.tool_snapshot_bytes = bytes;
        Ok(())
    }
}
