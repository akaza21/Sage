use sage_acp_protocol::lifecycle::ServerRequestState;

use super::{AdapterError, CodexAdapter, ExternalKind, ScopedExternalId};

impl CodexAdapter {
    pub(crate) fn resolve_external_permission(
        &mut self,
        request_id: &str,
    ) -> Result<Option<(ScopedExternalId, ScopedExternalId)>, AdapterError> {
        let Some(entry) = self
            .permission_requests
            .iter_mut()
            .find(|entry| entry.id == request_id)
        else {
            return Ok(None);
        };
        if !matches!(
            entry.state,
            ServerRequestState::Received | ServerRequestState::Asked
        ) {
            return Ok(None);
        }
        entry.state = ServerRequestState::Cancelled;
        let turn_id = entry.turn_id.clone();
        Ok(Some((
            self.scoped_turn(&turn_id)?,
            self.scoped_in_turn(ExternalKind::Request, &turn_id, request_id)?,
        )))
    }
}
