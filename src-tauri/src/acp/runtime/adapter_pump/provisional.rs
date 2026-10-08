use super::*;
use crate::acp::bindings::adapter::PROVISIONAL_TURN_HANDOFF_MILLIS;

impl<A: HarnessAdapter, J: AdapterBatchJournal, R: ResourceMapper> AdapterPump<A, J, R> {
    pub(super) fn refresh_provisional_deadline(&mut self, now_millis: i64) {
        if self.adapter.provisional_turn_pending() {
            self.provisional_turn_deadline_millis
                .get_or_insert(now_millis.saturating_add(PROVISIONAL_TURN_HANDOFF_MILLIS as i64));
        } else {
            self.provisional_turn_deadline_millis = None;
        }
    }

    pub(super) fn enforce_provisional_deadline(
        &mut self,
        now_millis: i64,
    ) -> Result<(), AdapterPumpError> {
        if self
            .provisional_turn_deadline_millis
            .is_some_and(|deadline| now_millis >= deadline)
        {
            self.teardown = true;
            return Err(AdapterPumpError::TeardownRequired);
        }
        Ok(())
    }
}
