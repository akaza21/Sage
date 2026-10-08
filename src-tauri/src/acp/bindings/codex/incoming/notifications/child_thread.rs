use serde_json::{Map, Value};

use crate::acp::intent::ExternalId;

use super::super::support::text;
use super::IngressError;

pub(super) fn native_id(
    item: &Map<String, Value>,
    params: &Map<String, Value>,
) -> Result<Option<ExternalId>, IngressError> {
    if item.get("type").and_then(Value::as_str) != Some("subAgentActivity") {
        return Ok(None);
    }
    item.get("agentThreadId")
        .map(|_| {
            let child = text(item, "agentThreadId", 512)?;
            if child == text(params, "threadId", 512)? {
                return Err(IngressError::Identity);
            }
            ExternalId::new("childThreadId", child.to_owned()).map_err(|_| IngressError::Identity)
        })
        .transpose()
}
