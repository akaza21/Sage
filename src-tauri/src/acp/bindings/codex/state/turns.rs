use crate::acp::{
    contract::{BoundedVec, SafeText},
    intent::{ExternalId, NormalizedPayload},
    lifecycle::{ItemLifecycle, TerminalUpdate, TurnState, classify_terminal},
    limits::LIMITS,
};

#[derive(Clone, Default, PartialEq)]
pub(in crate::acp::bindings::codex) struct SubagentLabels {
    entries: Vec<(SafeText<512>, SafeText<512>)>,
}

impl SubagentLabels {
    pub(in crate::acp::bindings::codex) fn apply(&mut self, payload: &mut NormalizedPayload) {
        let NormalizedPayload::Collaboration {
            label,
            agent_key,
            agent_keys,
            receiver_labels,
            ..
        } = payload
        else {
            return;
        };
        if let Some(key) = agent_key.as_ref().filter(|_| label.as_str() != "Subagent") {
            if let Some((_, existing)) = self.entries.iter_mut().find(|(entry, _)| entry == key) {
                *existing = label.clone();
            } else {
                if self.entries.len() == 300 {
                    self.entries.remove(0);
                }
                self.entries.push((key.clone(), label.clone()));
            }
        } else if let Some(key) = agent_key.as_ref()
            && let Some((_, existing)) = self.entries.iter().find(|(entry, _)| entry == key)
        {
            *label = existing.clone();
        }
        if agent_keys.as_slice().is_empty() {
            return;
        }
        let labels = agent_keys
            .as_slice()
            .iter()
            .map(|key| {
                SafeText::new(
                    "receiverLabel",
                    self.entries
                        .iter()
                        .find(|(entry, _)| entry == key)
                        .map(|(_, label)| label.as_str())
                        .unwrap_or(""),
                )
            })
            .collect::<Result<Vec<_>, _>>();
        if let Ok(labels) = labels
            && let Ok(bounded) = BoundedVec::new("receiverLabels", labels)
        {
            *receiver_labels = bounded;
        }
    }
}

use super::{
    AdapterError, CodexAdapter, CorrelationMatch, ExternalKind, ItemTerminal, RequestKind,
    ScopedExternalId, TERMINAL_ID_BYTES, TurnBinding, TurnTombstone,
};

impl CodexAdapter {
    pub(in crate::acp::bindings::codex) fn enrich_subagent_payload(
        &mut self,
        payload: &mut NormalizedPayload,
    ) {
        self.subagent_labels.apply(payload);
    }
    pub(crate) fn bind_thread(&mut self, value: &str) -> Result<ScopedExternalId, AdapterError> {
        let id = ExternalId::new("externalSessionId", value.to_owned())?;
        if let Some(current) = self.external_thread_id.as_ref()
            && current != &id
        {
            return Err(AdapterError::IdentityConflict);
        }
        self.external_thread_id = Some(id.clone());
        Ok(self.scoped(ExternalKind::Session, id))
    }

    pub(crate) fn accept_thread_started(&mut self, value: &str) -> Result<(), AdapterError> {
        let mut pending = self.pending.iter().filter_map(|(id, kind)| {
            matches!(kind, RequestKind::Start | RequestKind::Resume).then_some(*id)
        });
        let Some(_) = pending.next() else {
            return self.expect_thread(value);
        };
        if pending.next().is_some() {
            return Err(AdapterError::ForeignRequest);
        }
        self.bind_thread(value)?;
        Ok(())
    }

    pub(crate) fn expect_thread(&self, value: &str) -> Result<(), AdapterError> {
        match self.external_thread_id.as_ref() {
            Some(current) if current.as_str() == value => Ok(()),
            _ => Err(AdapterError::ForeignIdentity),
        }
    }

    pub(crate) fn observe_turn_started(
        &mut self,
        value: &str,
    ) -> Result<(ScopedExternalId, TurnBinding), AdapterError> {
        let id = ExternalId::new("externalTurnId", value.to_owned())?;
        if let Some(provisional) = self.provisional_turn.as_mut() {
            if self
                .previous_turn
                .as_ref()
                .is_some_and(|previous| previous.id == id)
            {
                return Err(AdapterError::IdentityConflict);
            }
            if provisional.started_id.is_some() {
                return Err(AdapterError::IdentityConflict);
            }
            if provisional.response_id != id {
                provisional.started_id = Some(id.clone());
                return Ok((
                    self.scoped(ExternalKind::Turn, id),
                    TurnBinding::Provisional,
                ));
            }
            let provisional = self.provisional_turn.take().expect("provisional turn");
            self.active_turn_client_message_id = Some(provisional.client_message_id);
            self.active_turn_id = Some(id);
            return Ok((self.scoped_turn(value)?, TurnBinding::NewlyActive));
        }
        if let Some(current) = self.active_turn_id.as_ref() {
            if current != &id {
                return Err(AdapterError::IdentityConflict);
            }
            return Ok((self.scoped_turn(value)?, TurnBinding::Active));
        }
        if self
            .previous_turn
            .as_ref()
            .is_some_and(|previous| previous.id == id)
        {
            return Ok((self.scoped_turn(value)?, TurnBinding::PreviousTerminal));
        }
        match self.turn_start_correlations.observe(&id)? {
            CorrelationMatch::Pending(client_message_id) => {
                self.retire_action_handoff()?;
                self.active_turn_client_message_id = Some(client_message_id);
                self.active_item_terminals.clear();
                self.active_unretried_error_fingerprints.clear();
                self.active_terminal_id_bytes = 0;
                self.active_turn_id = Some(id.clone());
                Ok((self.scoped_turn(value)?, TurnBinding::NewlyActive))
            }
            CorrelationMatch::Terminal {
                turn_id,
                client_message_id,
            } => Ok((
                self.scoped_correlated_turn(&turn_id, &client_message_id)?,
                TurnBinding::PreviousTerminal,
            )),
            CorrelationMatch::Active { .. } => Err(AdapterError::TurnIntentConflict),
        }
    }

    pub(crate) fn complete_turn_start(
        &mut self,
        request_id: u64,
        value: &str,
    ) -> Result<(ScopedExternalId, TurnBinding), AdapterError> {
        let turn_id = ExternalId::new("externalTurnId", value.to_owned())?;
        let correlation = self
            .turn_start_correlations
            .response(request_id, &turn_id)?;
        // Only a prompt Turn echoes the client message a provisional Turn
        // waits for.
        if self.pending_kind(request_id) == Some(RequestKind::TurnStart)
            && let Some(result) =
                self.provision_response_turn(request_id, &turn_id, &correlation)?
        {
            return Ok(result);
        }
        let result = match correlation {
            CorrelationMatch::Pending(client_message_id) => {
                if self.active_turn_id.as_ref() == Some(&turn_id)
                    || self
                        .previous_turn
                        .as_ref()
                        .is_some_and(|previous| previous.id == turn_id)
                {
                    return Err(AdapterError::IdentityConflict);
                }
                self.active_turn_client_message_id = Some(client_message_id);
                self.active_item_terminals.clear();
                self.active_unretried_error_fingerprints.clear();
                self.active_terminal_id_bytes = 0;
                self.active_turn_id = Some(turn_id);
                (self.scoped_turn(value)?, TurnBinding::NewlyActive)
            }
            CorrelationMatch::Active {
                turn_id,
                client_message_id,
            } => (
                self.scoped_correlated_turn(&turn_id, &client_message_id)?,
                TurnBinding::Active,
            ),
            CorrelationMatch::Terminal {
                turn_id,
                client_message_id,
            } => (
                self.scoped_correlated_turn(&turn_id, &client_message_id)?,
                TurnBinding::PreviousTerminal,
            ),
        };
        self.finish_turn_start_response(request_id)?;
        Ok(result)
    }

    pub(crate) fn expect_turn(&self, value: &str) -> Result<(), AdapterError> {
        if self
            .active_turn_id
            .as_ref()
            .is_some_and(|current| current.as_str() == value)
            || self
                .previous_turn
                .as_ref()
                .is_some_and(|previous| previous.id.as_str() == value)
        {
            Ok(())
        } else {
            Err(AdapterError::ForeignIdentity)
        }
    }

    pub(crate) fn has_pending_turn_start(&self) -> bool {
        self.turn_start_correlations.has_pending()
    }

    pub(crate) fn has_previous_turn(&self) -> bool {
        self.previous_turn.is_some()
    }

    pub(crate) fn turn_is_terminal(&self, value: &str) -> bool {
        self.previous_turn
            .as_ref()
            .is_some_and(|previous| previous.id.as_str() == value)
    }

    pub(crate) fn item_is_terminal(&self, turn: &str, id: &str) -> bool {
        self.item_terminals(turn)
            .is_some_and(|terminals| terminals.iter().any(|entry| entry.id == id))
    }

    /// The identity of a turn this adapter knows, carrying its own turn id
    /// and client message id, which the pump needs to match the turn.
    pub(crate) fn scoped_turn(&self, turn: &str) -> Result<ScopedExternalId, AdapterError> {
        self.scoped_in_turn(ExternalKind::Turn, turn, turn)
    }

    pub(crate) fn scoped_in_turn(
        &self,
        kind: ExternalKind,
        turn: &str,
        value: &str,
    ) -> Result<ScopedExternalId, AdapterError> {
        self.expect_turn(turn)?;
        let mut scoped = self.scoped(kind, ExternalId::new("externalId", value.to_owned())?);
        scoped.turn_id = Some(ExternalId::new("externalTurnId", turn.to_owned())?);
        scoped.turn_client_message_id = Some(
            if self
                .active_turn_id
                .as_ref()
                .is_some_and(|active| active.as_str() == turn)
            {
                self.active_turn_client_message_id.as_ref()
            } else {
                self.previous_turn
                    .as_ref()
                    .filter(|previous| previous.id.as_str() == turn)
                    .map(|previous| &previous.client_message_id)
            }
            .ok_or(AdapterError::TurnIntentConflict)?
            .clone(),
        );
        Ok(scoped)
    }

    pub(crate) fn accept_turn_terminal(
        &mut self,
        id: &str,
        state: TurnState,
    ) -> Result<TerminalUpdate, AdapterError> {
        if let Some(previous) = self
            .previous_turn
            .as_ref()
            .filter(|previous| previous.id.as_str() == id)
        {
            return Ok(classify_terminal(Some(&previous.state), &state));
        }
        let active = self
            .active_turn_id
            .as_ref()
            .filter(|active| active.as_str() == id)
            .ok_or(AdapterError::ForeignIdentity)?
            .clone();
        let client_message_id = self
            .active_turn_client_message_id
            .take()
            .ok_or(AdapterError::TurnIntentConflict)?;
        self.turn_start_correlations.mark_terminal(&active);
        self.previous_turn = Some(TurnTombstone {
            id: active,
            client_message_id,
            state,
            item_terminals: std::mem::take(&mut self.active_item_terminals),
            terminal_id_bytes: std::mem::take(&mut self.active_terminal_id_bytes),
            unretried_error_fingerprints: std::mem::take(
                &mut self.active_unretried_error_fingerprints,
            ),
        });
        self.active_turn_id = None;
        self.active_turn_user_stop = false;
        self.active_turn_plan = None;
        self.deferred_interrupt = false;
        Ok(TerminalUpdate::Apply)
    }

    pub(crate) fn accept_item_terminal(
        &mut self,
        turn: &str,
        id: &str,
        state: ItemLifecycle,
    ) -> Result<TerminalUpdate, AdapterError> {
        let (terminals, retained_bytes, allow_new) = if self
            .active_turn_id
            .as_ref()
            .is_some_and(|active| active.as_str() == turn)
        {
            (
                &mut self.active_item_terminals,
                &mut self.active_terminal_id_bytes,
                true,
            )
        } else if let Some(previous) = self
            .previous_turn
            .as_mut()
            .filter(|previous| previous.id.as_str() == turn)
        {
            (
                &mut previous.item_terminals,
                &mut previous.terminal_id_bytes,
                false,
            )
        } else {
            return Err(AdapterError::ForeignIdentity);
        };
        if let Some(existing) = terminals.iter().find(|entry| entry.id == id) {
            return Ok(classify_terminal(Some(&existing.state), &state));
        }
        if !allow_new {
            return Err(AdapterError::ForeignIdentity);
        }
        if terminals.len() >= LIMITS.wire.items_per_turn as usize
            || retained_bytes.saturating_add(id.len()) > TERMINAL_ID_BYTES
        {
            return Err(AdapterError::TerminalLedgerFull);
        }
        *retained_bytes += id.len();
        terminals.push(ItemTerminal {
            id: id.to_owned(),
            state,
        });
        Ok(TerminalUpdate::Apply)
    }

    fn item_terminals(&self, turn: &str) -> Option<&[ItemTerminal]> {
        if self
            .active_turn_id
            .as_ref()
            .is_some_and(|active| active.as_str() == turn)
        {
            Some(&self.active_item_terminals)
        } else {
            self.previous_turn
                .as_ref()
                .filter(|previous| previous.id.as_str() == turn)
                .map(|previous| previous.item_terminals.as_slice())
        }
    }

    fn scoped_correlated_turn(
        &self,
        turn_id: &ExternalId,
        client_message_id: &crate::acp::contract::RequiredText<512>,
    ) -> Result<ScopedExternalId, AdapterError> {
        let mut scoped = self.scoped(ExternalKind::Turn, turn_id.clone());
        scoped.turn_id = Some(turn_id.clone());
        scoped.turn_client_message_id = Some(client_message_id.clone());
        Ok(scoped)
    }
}
