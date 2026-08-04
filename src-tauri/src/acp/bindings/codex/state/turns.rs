use crate::acp::{
    intent::ExternalId,
    lifecycle::{ItemLifecycle, TerminalUpdate, TurnState, classify_terminal},
    limits::LIMITS,
};

use super::{
    AdapterError, CodexAdapter, ExternalKind, ItemTerminal, ScopedExternalId, TERMINAL_ID_BYTES,
    TurnBinding, TurnTombstone,
};

impl CodexAdapter {
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
        if let Some(current) = self.active_turn_id.as_ref() {
            if current != &id {
                return Err(AdapterError::IdentityConflict);
            }
            return Ok((
                self.scoped_in_turn(ExternalKind::Turn, value, value)?,
                TurnBinding::Active,
            ));
        }
        if self
            .previous_turn
            .as_ref()
            .is_some_and(|previous| previous.id == id)
        {
            return Ok((
                self.scoped_in_turn(ExternalKind::Turn, value, value)?,
                TurnBinding::PreviousTerminal,
            ));
        }
        if self.pending_turn_start.is_none() {
            return Err(AdapterError::TurnIntentConflict);
        }
        let client_message_id = self
            .pending_turn_start
            .as_ref()
            .map(|intent| intent.client_message_id.clone())
            .ok_or(AdapterError::TurnIntentConflict)?;
        self.pending_turn_start = None;
        self.active_turn_client_message_id = Some(client_message_id);
        self.active_item_terminals.clear();
        self.active_terminal_id_bytes = 0;
        self.active_turn_id = Some(id.clone());
        Ok((
            self.scoped_in_turn(ExternalKind::Turn, value, value)?,
            TurnBinding::Active,
        ))
    }

    pub(crate) fn complete_turn_start(
        &mut self,
        request_id: u64,
        value: &str,
    ) -> Result<(ScopedExternalId, TurnBinding), AdapterError> {
        if self
            .pending_turn_start
            .as_ref()
            .is_some_and(|intent| intent.request_id == request_id)
        {
            return self.observe_turn_started(value);
        }
        if self
            .active_turn_id
            .as_ref()
            .is_some_and(|turn| turn.as_str() == value)
        {
            return Ok((
                self.scoped_in_turn(ExternalKind::Turn, value, value)?,
                TurnBinding::Active,
            ));
        }
        if self
            .previous_turn
            .as_ref()
            .is_some_and(|turn| turn.id.as_str() == value)
        {
            return Ok((
                self.scoped_in_turn(ExternalKind::Turn, value, value)?,
                TurnBinding::PreviousTerminal,
            ));
        }
        Err(AdapterError::TurnIntentConflict)
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

    pub(crate) fn turn_is_terminal(&self, value: &str) -> bool {
        self.previous_turn
            .as_ref()
            .is_some_and(|previous| previous.id.as_str() == value)
    }

    pub(crate) fn item_is_terminal(&self, turn: &str, id: &str) -> bool {
        self.item_terminals(turn)
            .is_some_and(|terminals| terminals.iter().any(|entry| entry.id == id))
    }

    pub(crate) fn scoped_id(
        &self,
        kind: ExternalKind,
        value: &str,
    ) -> Result<ScopedExternalId, AdapterError> {
        Ok(self.scoped(kind, ExternalId::new("externalId", value.to_owned())?))
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
        self.previous_turn = Some(TurnTombstone {
            id: active,
            client_message_id,
            state,
            item_terminals: std::mem::take(&mut self.active_item_terminals),
            terminal_id_bytes: std::mem::take(&mut self.active_terminal_id_bytes),
        });
        self.active_turn_id = None;
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
}
