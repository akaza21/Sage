//! Harness questions that carry an auto-resolution window resolve
//! themselves, without answers, once the window passes. The pump journals
//! the decline first, then sends the same reply a user cancel would.

use super::*;

impl<A: HarnessAdapter, J: AdapterBatchJournal, R: ResourceMapper> AdapterPump<A, J, R> {
    /// The earliest instant an undecided question request resolves itself.
    pub(crate) fn next_auto_resolution_deadline_millis(&self) -> Option<i64> {
        self.pending_permissions
            .iter()
            .filter(|pending| pending.settles_as.is_none())
            .filter_map(|pending| pending.auto_resolve_at_millis)
            .min()
    }

    /// Declines one question request whose window has passed. Returns
    /// whether a reply was queued. A request the user resolved first keeps
    /// the user's decision: the store refuses the decline and nothing is sent.
    pub(crate) async fn auto_resolve_due(
        &mut self,
        now_millis: i64,
    ) -> Result<bool, AdapterPumpError> {
        self.ensure_live()?;
        let Some(pending) = self.pending_permissions.iter_mut().find(|pending| {
            pending.settles_as.is_none()
                && pending
                    .auto_resolve_at_millis
                    .is_some_and(|deadline| deadline <= now_millis)
        }) else {
            return Ok(false);
        };
        pending.auto_resolve_at_millis = None;
        let intent = pending.intent.clone();
        match self
            .journal
            .transition_permission(&intent, ServerRequestState::DeclinePending, now_millis)
            .await
        {
            Ok(revision) => self.publish_revision(revision)?,
            Err(PumpJournalError::Rejected) => return Ok(false),
            Err(error @ PumpJournalError::CommitUnknown) => {
                self.teardown = true;
                return Err(AdapterPumpError::Journal(error));
            }
        }
        self.resolve_permission(
            &intent.request_id,
            crate::acp::runtime::permissions::PermissionDecision::Deny,
            now_millis,
        )
        .await?;
        Ok(true)
    }
}
