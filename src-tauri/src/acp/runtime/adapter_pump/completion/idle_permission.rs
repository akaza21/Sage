//! A permission request the harness settles after its Turn ended.
//!
//! A harness may stop waiting for a permission answer as its Turn is
//! cancelled and name the abandoned request only after the Turn's terminal is
//! durable. Turn terminalisation fails the permission item but leaves the
//! request row open, so the harness's resolution is committed here against
//! the last Turn; a row it does not name fails the commit closed.

use super::*;

impl<A: HarnessAdapter, J: AdapterBatchJournal, R: ResourceMapper> CompletedPumpParts<A, J, R> {
    pub(super) async fn commit_idle_permission_resolution(
        &mut self,
        id: crate::acp::bindings::adapter::AdapterIdentity,
        external_request_id: ExternalId,
        state: crate::acp::lifecycle::ServerRequestState,
        frame_bytes: usize,
        now_millis: i64,
    ) -> Result<(), AdapterPumpError> {
        let intent = &self.session_intent;
        let this_session = id.binding_digest == intent.binding_digest
            && id.runtime_epoch == intent.runtime_epoch
            && id.session_id == intent.session_id
            && id.external_id == intent.external_session_id;
        if !this_session
            || id.turn_id.is_none()
            || id.turn_client_message_id.is_none()
            || !state.is_terminal()
        {
            return Err(AdapterPumpError::Frame);
        }
        let request_id = super::super::normalize::scoped_permission_id(
            &self.session_intent.session_id,
            self.session_intent.runtime_epoch,
            &external_request_id,
        )?;
        let scope = TurnScope {
            project_id: self.session_intent.project_id.clone(),
            chat_id: self.session_intent.chat_id.clone(),
            session_id: self.session_intent.session_id.clone(),
            runtime_epoch: self.session_intent.runtime_epoch,
            turn_id: self.terminal_cursor.turn_id().clone(),
            binding_digest: hex(&self.session_intent.binding_digest),
        };
        let receipt = self
            .journal
            .commit(
                vec![DurableAdapterEvent::PermissionResolved {
                    scope,
                    request_id,
                    state,
                    now_millis,
                }],
                frame_bytes.max(1),
            )
            .await
            .map_err(AdapterPumpError::Journal)?;
        if receipt.applied() {
            let _ = self
                .publisher
                .publish_committed(crate::acp::intent::ProjectionChangedV1 {
                    project_id: self.session_intent.project_id.clone(),
                    chat_id: self.session_intent.chat_id.clone(),
                    session_id: self.session_intent.session_id.clone(),
                    projection_revision: receipt.projection_revision,
                    child_projection_epoch: 0,
                });
        }
        Ok(())
    }
}
