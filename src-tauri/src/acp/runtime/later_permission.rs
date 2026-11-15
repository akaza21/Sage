use crate::acp::bindings::adapter::{AdapterEgress, EgressToken};
use sage_acp_protocol::{
    contract::SageServerRequestId, intent::ExternalId, permissions::PermissionDecision,
};

/// Turns an adapter-owned permission reply into the common durable-decision
/// egress shape. Adapters retain ownership of protocol bytes and pending
/// request correlation; this module owns the token semantics only.
pub(crate) fn resolve_later_permission(
    request_id: &SageServerRequestId,
    decision: &PermissionDecision,
    reply: Option<AdapterEgress>,
) -> Result<Option<AdapterEgress>, LaterPermissionError> {
    if *decision == PermissionDecision::Ask {
        return Ok(None);
    }
    let reply = reply.ok_or(LaterPermissionError::PendingRequestMiss)?;
    let external_request_id = ExternalId::new("permissionRequest", request_id.as_str().to_owned())
        .map_err(|_| LaterPermissionError::PendingRequestMiss)?;
    let token = if decision.resolves_allowed() {
        EgressToken::permission_allow(external_request_id)
    } else {
        EgressToken::mandatory_deny(external_request_id, false)
    };
    Ok(Some(AdapterEgress::new(reply.into_frame(), token)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum LaterPermissionError {
    #[error("there is no exact pending request for this later permission decision")]
    PendingRequestMiss,
}

#[cfg(test)]
mod tests;
