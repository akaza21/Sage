use serde_json::{Value, json};

use crate::acp::{
    bindings::adapter::{AdapterBatch, AdapterEgress, AdapterEvent as SharedEvent, EgressToken},
    contract::RequiredText,
    lifecycle::{ItemLifecycle, ToolState},
    runtime::host_browse::{
        admit_browse_url, browse_tool_payload, host_browse_session_matches,
        parse_host_browse_request,
    },
    transport::OutboundFrame,
};

use super::*;

impl ClaudeRuntime {
    pub(super) fn ingest_browse_host(
        &mut self,
        value: Value,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        let wire_id = value.get("id").cloned().ok_or(AdapterRuntimeError::Frame)?;
        let params = value.get("params").and_then(Value::as_object);
        let request = parse_host_browse_request(
            value.get("method").and_then(Value::as_str).unwrap_or(""),
            params,
        )
        .ok_or(AdapterRuntimeError::Frame)?;
        let Some(active) = self.active.as_ref() else {
            return browse_reply(wire_id, None);
        };
        if active.turn.is_none()
            || !host_browse_session_matches(&request, active.external_id.as_str())
        {
            return browse_reply(wire_id, None);
        }
        let Some(href) = admit_browse_url(&request.url) else {
            return browse_reply(wire_id, None);
        };
        let external_key = RequiredText::new(
            "externalItemId",
            format!("sage.browse:{}", wire_id.as_str().unwrap_or("host")),
        )
        .map_err(|_| AdapterRuntimeError::Frame)?;
        let item = SharedEvent::Item {
            id: self.item_identity(active, external_key)?,
            lifecycle: ItemLifecycle::Started,
            tool_state: Some(ToolState::Pending),
            payload: browse_tool_payload(&href).map_err(|_| AdapterRuntimeError::Frame)?,
        };
        let mut batch = AdapterBatchBuilder::new();
        batch.push_event(item)?;
        batch.push_egress(browse_opened_egress(wire_id, &href)?)?;
        Ok(batch.finish())
    }
}

fn browse_reply(wire_id: Value, href: Option<&str>) -> Result<AdapterBatch, AdapterRuntimeError> {
    AdapterBatch::new(
        Vec::new(),
        vec![browse_opened_egress(wire_id, href.unwrap_or(""))?],
    )
}

fn browse_opened_egress(wire_id: Value, href: &str) -> Result<AdapterEgress, AdapterRuntimeError> {
    let opened = !href.is_empty();
    let bytes = serde_json::to_vec(&json!({
        "jsonrpc": "2.0",
        "id": wire_id,
        "result": { "opened": opened, "href": href }
    }))
    .map_err(|_| AdapterRuntimeError::Frame)?;
    let frame = OutboundFrame::new(bytes).map_err(|_| AdapterRuntimeError::Frame)?;
    Ok(AdapterEgress::new(
        frame,
        EgressToken::protocol_notification(),
    ))
}
