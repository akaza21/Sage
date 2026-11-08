//! `$/cancel_request` from the adapter for a permission request it asked.
//!
//! With ACP SDK 2.x (claude-agent-acp 0.101.0) the adapter stops waiting for
//! the permission answer when its turn is cancelled: it sends
//! `$/cancel_request` naming the request and may resolve the prompt
//! `cancelled` before Sage answers. SDK 1.x (0.76.0) sends the same
//! notification but keeps waiting for the answer. In both cases an open
//! permission card is settled as cancelled by the harness, and Sage answers
//! `cancelled` so a waiting 1.x adapter is released; a 2.x adapter drops the
//! late answer quietly.

use super::super::*;

impl ClaudeRuntime {
    pub(in crate::acp::bindings::claude::runtime) fn cancel_request_notification(
        &mut self,
        value: &Value,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        let wire_id = value
            .pointer("/params/requestId")
            .filter(|id| id.is_string() || id.is_u64() || id.is_i64())
            .ok_or(AdapterRuntimeError::Frame)?;
        let Some((request_id, record)) = self
            .denials
            .iter()
            .find(|(_, record)| &record.wire_id == wire_id)
        else {
            // Not a permission request: Sage sends no other cancellable request.
            return AdapterBatch::new(
                vec![SharedEvent::Unsupported(unsupported("$/cancel_request")?)],
                Vec::new(),
            );
        };
        // Sage already answered (or is answering) it; nothing is open.
        if record.state != DenialState::Pending {
            return AdapterBatch::new(vec![SharedEvent::IgnoredLate], Vec::new());
        }
        let Some(active) = self.active.as_ref().filter(|active| active.turn.is_some()) else {
            return AdapterBatch::new(vec![SharedEvent::IgnoredLate], Vec::new());
        };
        let external_request_id = ExternalId::new("permissionRequest", request_id.clone())
            .map_err(|_| AdapterRuntimeError::Frame)?;
        let server_request = crate::acp::contract::SageServerRequestId::parse(request_id.as_str())
            .map_err(|_| AdapterRuntimeError::Frame)?;
        let answer = cancelled_permission(&server_request, &record.wire_id)?;
        AdapterBatch::new(
            vec![SharedEvent::PermissionResolved {
                turn_id: self.turn_identity(active)?,
                external_request_id,
                state: crate::acp::lifecycle::ServerRequestState::Cancelled,
            }],
            vec![answer],
        )
    }
}
