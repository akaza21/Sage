use crate::acp::{
    lifecycle::ServerRequestState, limits::LIMITS, runtime::permissions::PermissionDecision,
};

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
            let allow_response_queued = existing.state == ServerRequestState::ResponseQueued
                && existing.response.is_empty();
            if existing.fingerprint != fingerprint
                || existing.method != method
                || existing.turn_id != turn_id
                || existing.item_id != item_id
                || (!allow_response_queued
                    && !matches!(
                        existing.state,
                        ServerRequestState::AllowPending
                            | ServerRequestState::AllowCommitted
                            | ServerRequestState::ResolvedAllowed
                    )
                    && existing.response != response)
            {
                return Err(AdapterError::PermissionTeardownRequired);
            }
            return Ok(match existing.state {
                ServerRequestState::Received
                | ServerRequestState::Asked
                | ServerRequestState::AllowPending
                | ServerRequestState::AllowCommitted
                | ServerRequestState::ResolvedAllowed => PermissionRegistration::DuplicatePending,
                ServerRequestState::ResponseQueued if allow_response_queued => {
                    PermissionRegistration::DuplicatePending
                }
                ServerRequestState::DeclinePending => PermissionRegistration::DuplicatePending,
                ServerRequestState::DeclineCommitted
                | ServerRequestState::ResponseQueued
                | ServerRequestState::ResolvedDeclined => {
                    PermissionRegistration::DuplicateCommitted(existing.response.clone())
                }
            });
        }
        if self.permission_requests.len() >= LIMITS.queues.server_requests_per_session as usize {
            let Some(index) = self.permission_requests.iter().position(|entry| {
                matches!(
                    entry.state,
                    ServerRequestState::ResolvedAllowed | ServerRequestState::ResolvedDeclined
                )
            }) else {
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

    pub(crate) fn advance_permission(
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

    pub(crate) fn advance_permission_decline(
        &mut self,
        request_id: &str,
        next: ServerRequestState,
    ) -> Result<(), AdapterError> {
        self.advance_permission(request_id, next)
    }

    pub(crate) fn resolve_pending_permission(
        &mut self,
        request_id: &str,
        decision: PermissionDecision,
    ) -> Result<Option<(String, Vec<u8>)>, AdapterError> {
        let Some(entry) = self
            .permission_requests
            .iter_mut()
            .find(|entry| entry.id == request_id)
        else {
            return Err(AdapterError::PermissionTeardownRequired);
        };
        if !matches!(
            entry.state,
            ServerRequestState::Received | ServerRequestState::Asked
        ) {
            return Err(AdapterError::PermissionTeardownRequired);
        }
        match decision {
            PermissionDecision::Ask => {
                entry.state = ServerRequestState::Asked;
                Ok(None)
            }
            PermissionDecision::AllowOnce => {
                entry.state = ServerRequestState::AllowPending;
                entry.state = ServerRequestState::AllowCommitted;
                // An empty retained response marks the shared queued state as
                // the allow path, so a duplicate cannot replay decline bytes.
                entry.response.clear();
                Ok(Some((entry.id.clone(), Vec::new())))
            }
            PermissionDecision::Deny => {
                entry.state = ServerRequestState::DeclinePending;
                Ok(Some((entry.id.clone(), entry.response.clone())))
            }
        }
    }
}
