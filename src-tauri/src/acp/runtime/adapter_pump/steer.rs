//! A steer the harness accepts becomes a durable user item in the active
//! Turn, at the place its accept arrives in the Turn's item order. Bindings
//! drop their own echoes of the steered message, so this is its only item.

use sha2::{Digest, Sha256};

use super::{coalescer::NormalizedFact, payload::payload_measure, *};
use sage_acp_protocol::contract::{ContractError, SafeText};

/// The steered message's durable identity and payload. The identity derives
/// from the client message id alone, so a retried steer or a repeated accept
/// lands on the one stored item instead of adding a second.
pub(crate) fn steered_prompt_item(
    prompt: &crate::acp::transcript_quote::Prompt,
    client_message_id: &RequiredText<512>,
) -> Result<(ExternalId, NormalizedPayload), ContractError> {
    let digest = Sha256::digest(client_message_id.as_str().as_bytes());
    let id = ExternalId::new("steeredMessageId", format!("sage-steer-{}", hex(&digest)))?;
    let text = SafeText::new("steeredMessage", prompt.display_text())?;
    Ok((id, NormalizedPayload::UserPrompt { text }))
}

impl<A: HarnessAdapter, J: AdapterBatchJournal, R: ResourceMapper> AdapterPump<A, J, R> {
    /// The pending steer's item once the harness accepts it; a rejected,
    /// stale or post-terminal accept journals nothing.
    pub(super) fn steered_item(
        &mut self,
        request_id: u64,
        now_millis: i64,
        terminal: Option<TurnState>,
    ) -> Result<Option<NormalizedFact>, AdapterPumpError> {
        if terminal.is_some() || self.pending_steer_request != Some(request_id) {
            return Ok(None);
        }
        let Some(item) = self.pending_steer_item.take() else {
            return Ok(None);
        };
        let (external_item_id, payload) = *item;
        let measured = payload_measure(&payload)?;
        Ok(Some(NormalizedFact::new(
            DurableAdapterEvent::Item {
                scope: self.scope(),
                external_item_id,
                lifecycle: ItemLifecycle::Completed,
                tool_state: None,
                payload,
                payload_digest: measured.digest,
                now_millis,
            },
            measured.bytes,
        )))
    }
}
