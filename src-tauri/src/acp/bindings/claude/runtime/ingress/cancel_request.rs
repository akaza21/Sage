//! `$/cancel_request` from the adapter for a permission request it asked.
//!
//! When its turn is cancelled the adapter stops waiting for the permission
//! answer and sends `$/cancel_request` naming the request. With ACP SDK 2.x
//! (claude-agent-acp 0.101.0) the SDK itself abandons the request; 0.76.0
//! (SDK 1.x) races the permission request against the turn's abort signal
//! locally (`acp-agent.js` 5150-5154, `raceWithAbort` at 571), so it also
//! stops waiting. Either way the prompt may resolve `cancelled` before Sage
//! answers. An open permission card in a running Turn is settled as cancelled
//! by the harness, and Sage still answers `cancelled` so the request is closed
//! on the wire; the adapter drops that late answer quietly.

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
        let external_request_id = ExternalId::new("permissionRequest", request_id.clone())
            .map_err(|_| AdapterRuntimeError::Frame)?;
        let Some(active) = self.active.as_ref().filter(|active| active.turn.is_some()) else {
            // The Turn already ended and the adapter no longer waits for an
            // answer. The request is settled as cancelled on the Turn it
            // opened in, so its durable row is terminal, and the record is
            // dropped, so a later answer from the user is a typed miss.
            let turn_id = record.turn.clone();
            let request_id = request_id.clone();
            self.denials.remove(&request_id);
            return AdapterBatch::new(
                vec![SharedEvent::PermissionResolved {
                    turn_id,
                    external_request_id,
                    state: crate::acp::lifecycle::ServerRequestState::Cancelled,
                }],
                Vec::new(),
            );
        };
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
