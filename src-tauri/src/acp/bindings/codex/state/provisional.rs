use crate::acp::{bindings::adapter::PROVISIONAL_TURN_HANDOFF_MILLIS, intent::ExternalId};

use super::{
    AdapterError, CodexAdapter, CorrelationMatch, ExternalKind, ProvisionalTurn, ScopedExternalId,
    TurnBinding,
};

impl CodexAdapter {
    pub(super) fn provision_response_turn(
        &mut self,
        request_id: u64,
        turn_id: &ExternalId,
        correlation: &CorrelationMatch,
    ) -> Result<Option<(ScopedExternalId, TurnBinding)>, AdapterError> {
        if !self.active_observed_goal() {
            return Ok(None);
        }
        let CorrelationMatch::Pending(client_message_id) = correlation else {
            return Ok(None);
        };
        if self.provisional_turn.is_some()
            || self.active_turn_id.is_some()
            || self
                .previous_turn
                .as_ref()
                .is_some_and(|previous| previous.id == *turn_id)
        {
            return Err(AdapterError::IdentityConflict);
        }
        self.provisional_turn = Some(ProvisionalTurn {
            response_id: turn_id.clone(),
            started_id: None,
            client_message_id: client_message_id.clone(),
            deadline: std::time::Instant::now()
                + std::time::Duration::from_millis(PROVISIONAL_TURN_HANDOFF_MILLIS),
        });
        self.finish_turn_start_response(request_id)?;
        Ok(Some((
            self.scoped(ExternalKind::Turn, turn_id.clone()),
            TurnBinding::Provisional,
        )))
    }

    pub(crate) fn provisional_expired(&self) -> bool {
        self.provisional_turn
            .as_ref()
            .is_some_and(|pending| std::time::Instant::now() >= pending.deadline)
    }

    pub(crate) fn has_provisional_turn(&self) -> bool {
        self.provisional_turn.is_some()
    }

    pub(crate) fn verify_provisional_user_message(
        &mut self,
        turn_id: &str,
        client_id: &str,
    ) -> Result<ScopedExternalId, AdapterError> {
        let pending = self
            .provisional_turn
            .as_ref()
            .ok_or(AdapterError::TurnIntentConflict)?;
        if pending
            .started_id
            .as_ref()
            .is_none_or(|id| id.as_str() != turn_id)
            || pending.client_message_id.as_str() != client_id
        {
            return Err(AdapterError::IdentityConflict);
        }
        let pending = self.provisional_turn.take().expect("provisional turn");
        self.active_turn_client_message_id = Some(pending.client_message_id);
        self.active_turn_id = pending.started_id;
        self.scoped_turn(turn_id)
    }
}
