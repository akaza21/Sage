//! Harness requests that wait for the user: admitting them to the journal
//! for the pump's active turn, declining the ones the chat will never show,
//! and recording how the harness resolved them.

use sha2::{Digest, Sha256};

use super::*;

/// A harness request as the adapter reported it, before admission.
pub(super) struct PermissionRequestEvent {
    pub(super) turn_id: crate::acp::bindings::adapter::AdapterIdentity,
    pub(super) item_id: crate::acp::bindings::adapter::AdapterIdentity,
    pub(super) external_request_id: ExternalId,
    pub(super) payload: NormalizedPayload,
    pub(super) request_payload_digest: [u8; 32],
}

impl<A: HarnessAdapter, J: AdapterBatchJournal, R: ResourceMapper> AdapterPump<A, J, R> {
    /// Journals a request for the active turn, or reports and declines one
    /// that does not belong to it.
    pub(super) fn permission_request(
        &mut self,
        event: PermissionRequestEvent,
        terminal: Option<TurnState>,
        now_millis: i64,
    ) -> Result<Option<NormalizedFact>, AdapterPumpError> {
        let PermissionRequestEvent {
            turn_id,
            item_id,
            external_request_id,
            payload,
            request_payload_digest,
        } = event;
        let unadmitted = if !self.exact_turn(&turn_id) {
            Some(UnadmittedRequest::ForeignTurn)
        } else if !self.exact_turn(&item_id) {
            Some(UnadmittedRequest::ForeignItem)
        } else if terminal.is_some() {
            Some(UnadmittedRequest::TerminalTurn)
        } else {
            None
        };
        if let Some(reason) = unadmitted {
            tracing::warn!(
                acp_unadmitted_request = reason.as_str(),
                external_request_id = external_request_id.as_str(),
                "adapter pump did not admit a harness request"
            );
            self.decline_unadmitted(&external_request_id)?;
            return Ok(None);
        }
        let request_id = self.scoped_permission_id(&external_request_id)?;
        let (normalized_payload_json, measured) = payload_json(&payload)?;
        // Paired with the resolve log: a second request for one harness
        // action shows here as a second admission.
        tracing::info!(
            acp_request_admitted = request_id.as_str(),
            external_request_id = external_request_id.as_str(),
            "adapter pump admitted a harness request"
        );
        Ok(Some(NormalizedFact::new(
            DurableAdapterEvent::PermissionRequest {
                intent: PermissionIntent {
                    request_id,
                    project_id: self.intent.project_id.clone(),
                    chat_id: self.intent.chat_id.clone(),
                    session_id: self.intent.session_id.clone(),
                    turn_id: self.intent.active_turn.turn_id.clone(),
                    runtime_epoch: self.intent.runtime_epoch,
                    binding_digest: hex(&self.intent.binding_digest),
                    external_request_id,
                    kind: RequiredText::new("requestKind", "permission")
                        .map_err(|_| AdapterPumpError::Intent)?,
                    payload_digest: hex(&request_payload_digest),
                    normalized_payload_digest: measured.digest,
                    normalized_payload_json,
                },
                now_millis,
            },
            measured.bytes,
        )))
    }

    /// Records the harness settling a journaled request on its own.
    pub(super) fn permission_resolved(
        &self,
        external_request_id: ExternalId,
        state: sage_acp_protocol::lifecycle::ServerRequestState,
        now_millis: i64,
    ) -> Result<Option<NormalizedFact>, AdapterPumpError> {
        let request_id = self.scoped_permission_id(&external_request_id)?;
        Ok(Some(NormalizedFact::new(
            DurableAdapterEvent::PermissionResolved {
                scope: self.scope(),
                request_id,
                state,
                now_millis,
            },
            external_request_id.as_str().len(),
        )))
    }

    /// Answers a request the chat will never show with the harness's own
    /// decline, so the harness does not wait on it forever.
    fn decline_unadmitted(
        &mut self,
        external_request_id: &ExternalId,
    ) -> Result<(), AdapterPumpError> {
        let request_id = SageServerRequestId::parse(external_request_id.as_str())
            .map_err(|_| AdapterPumpError::Intent)?;
        let egress = match self.adapter.command(AdapterCommand::ResolvePermission {
            request_id,
            decision: sage_acp_protocol::permissions::PermissionDecision::Deny,
        }) {
            Ok(egress) => egress,
            Err(error) => {
                tracing::warn!(
                    ?error,
                    external_request_id = external_request_id.as_str(),
                    "adapter pump could not decline an unadmitted harness request"
                );
                return Ok(());
            }
        };
        if egress.token().is_mandatory()
            && self
                .adapter
                .commit_mandatory_egress(egress.token())
                .is_err()
        {
            self.teardown = true;
            return Err(AdapterPumpError::TeardownRequired);
        }
        self.enqueue(egress).map(drop)
    }

    fn scoped_permission_id(
        &self,
        external_request_id: &ExternalId,
    ) -> Result<SageServerRequestId, AdapterPumpError> {
        let session_digest = Sha256::digest(self.intent.session_id.as_str().as_bytes());
        let external_digest = Sha256::digest(external_request_id.as_str().as_bytes());
        SageServerRequestId::parse(format!(
            "permission:{session_digest:x}:{}:{external_digest:x}",
            self.intent.runtime_epoch.get()
        ))
        .map_err(|_| AdapterPumpError::Intent)
    }
}

/// Why the pump refused to journal a harness request that waits for the
/// user. Each refusal is reported and the request is declined.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UnadmittedRequest {
    /// The request's turn identity is not the pump's active turn.
    ForeignTurn,
    /// The request's item identity is not scoped to the pump's active turn.
    ForeignItem,
    /// The active turn already reached a terminal state in this batch.
    TerminalTurn,
}

impl UnadmittedRequest {
    const fn as_str(self) -> &'static str {
        match self {
            Self::ForeignTurn => "foreignTurn",
            Self::ForeignItem => "foreignItem",
            Self::TerminalTurn => "terminalTurn",
        }
    }
}
