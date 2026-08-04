use crate::acp::{lifecycle::ServerRequestState, limits::LIMITS};

use super::{
    AdapterError, CodexAdapter, ExternalKind, PermissionRegistration, PermissionRequestEntry,
};

impl CodexAdapter {
    pub(crate) fn register_permission_request(
        &mut self,
        request_id: &str,
        fingerprint: u64,
        method: &str,
        turn_id: &str,
        item_id: &str,
        response: &[u8],
    ) -> Result<PermissionRegistration, AdapterError> {
        self.expect_turn(turn_id)?;
        if let Some(existing) = self
            .permission_requests
            .iter()
            .find(|entry| entry.id == request_id)
        {
            if existing.fingerprint != fingerprint
                || existing.method != method
                || existing.turn_id != turn_id
                || existing.item_id != item_id
                || existing.response != response
            {
                return Err(AdapterError::PermissionTeardownRequired);
            }
            return Ok(match existing.state {
                ServerRequestState::Received | ServerRequestState::DeclinePending => {
                    PermissionRegistration::DuplicatePending
                }
                ServerRequestState::DeclineCommitted
                | ServerRequestState::ResponseQueued
                | ServerRequestState::ResolvedDeclined => {
                    PermissionRegistration::DuplicateCommitted(existing.response.clone())
                }
            });
        }
        if self.permission_requests.len() >= LIMITS.queues.server_requests_per_session as usize {
            let Some(index) = self
                .permission_requests
                .iter()
                .position(|entry| entry.state == ServerRequestState::ResolvedDeclined)
            else {
                return Err(AdapterError::PermissionTeardownRequired);
            };
            self.permission_requests.swap_remove(index);
        }
        let external = self.scoped_in_turn(ExternalKind::Request, turn_id, request_id)?;
        self.permission_requests.push(PermissionRequestEntry {
            id: request_id.to_owned(),
            fingerprint,
            method: method.to_owned(),
            turn_id: turn_id.to_owned(),
            item_id: item_id.to_owned(),
            response: response.to_vec(),
            state: ServerRequestState::Received,
        });
        Ok(PermissionRegistration::New(external))
    }

    pub(crate) fn advance_permission_decline(
        &mut self,
        request_id: &str,
        next: ServerRequestState,
    ) -> Result<(), AdapterError> {
        let entry = self
            .permission_requests
            .iter_mut()
            .find(|entry| entry.id == request_id)
            .ok_or(AdapterError::PermissionTeardownRequired)?;
        if entry.state == next {
            return Ok(());
        }
        if !entry.state.can_transition_to(next) {
            return Err(AdapterError::PermissionTeardownRequired);
        }
        entry.state = next;
        Ok(())
    }
}
