use serde_json::{Map, Value};

use sage_acp_protocol::lifecycle::{ItemLifecycle, TurnState};

use super::{CodexAdapter, IncomingEvent, IngressDisposition, IngressError, identity, text};

pub(super) fn verify_first_user_message(
    adapter: &mut CodexAdapter,
    params: &Map<String, Value>,
    requested: ItemLifecycle,
) -> Result<IngressDisposition, IngressError> {
    if requested != ItemLifecycle::Started {
        return Err(IngressError::Identity);
    }
    adapter
        .expect_thread(text(params, "threadId", 512)?)
        .map_err(identity)?;
    let turn = text(params, "turnId", 512)?;
    let item = params
        .get("item")
        .and_then(Value::as_object)
        .ok_or(IngressError::Malformed)?;
    if text(item, "type", 64)? != "userMessage" {
        return Err(IngressError::Identity);
    }
    let _ = text(item, "id", 512)?;
    let client_id = text(item, "clientId", 512)?;
    let id = adapter
        .verify_provisional_user_message(turn, client_id)
        .map_err(identity)?;
    Ok(IncomingEvent::Turn {
        id,
        state: TurnState::Running,
    }
    .into())
}
