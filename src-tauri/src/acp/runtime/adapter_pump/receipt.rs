use super::*;

impl<A: HarnessAdapter, J: AdapterBatchJournal, R: ResourceMapper> AdapterPump<A, J, R> {
    pub(super) fn accept_receipt(
        &mut self,
        receipt: &JournalReceipt,
        expected_terminal: Option<TurnState>,
    ) -> Result<(), AdapterPumpError> {
        let cursor = receipt.turn_cursor();
        if cursor.turn_id() != &self.intent.active_turn.turn_id
            || cursor.projection_revision() != receipt.projection_revision
            || expected_terminal.is_some_and(|state| cursor.state() != state)
        {
            return Err(AdapterPumpError::Intent);
        }
        if let Some(previous) = self.durable_turn_cursor.as_ref()
            && (cursor.revision() < previous.revision()
                || cursor.projection_revision() < previous.projection_revision()
                || (cursor.revision() > previous.revision()
                    && cursor.projection_revision() <= previous.projection_revision())
                || (cursor.revision() == previous.revision()
                    && (cursor.state() != previous.state()
                        || cursor.delivery_state() != previous.delivery_state())))
        {
            return Err(AdapterPumpError::Intent);
        }
        self.terminal = cursor.state().is_terminal().then_some(cursor.state());
        self.durable_turn_cursor = Some(cursor.clone());
        Ok(())
    }
}
