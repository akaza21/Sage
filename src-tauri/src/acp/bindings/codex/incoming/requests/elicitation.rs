//! MCP elicitations are answered `accept` (with typed form content), `decline`
//! or `cancel`, as ACP `elicitation/create` requests are.

use serde_json::{Map, Value};

use crate::acp::{bindings::standard_acp::elicitation::refused, intent::user_input::form};

use super::super::{
    IncomingEvent, IngressDisposition, IngressError, record_unprojected,
    support::{request_id_text, text},
};
use super::{CodexAdapter, PendingServerRequest, project_server_request, reply};

pub(in crate::acp::bindings::codex) const ELICITATION_METHOD: &str =
    "mcpServer/elicitation/request";

pub(super) fn request(
    adapter: &mut CodexAdapter,
    object: &Map<String, Value>,
    params: &Map<String, Value>,
) -> Result<IngressDisposition, IngressError> {
    let id = object.get("id").ok_or(IngressError::Malformed)?;
    if adapter.bound_thread_id().is_none() {
        return answer_now(id, refused(false));
    }
    // A subagent's MCP server asks on its own thread, which has no card here;
    // that is a refusal for that server, not a broken session.
    if adapter
        .expect_thread(text(params, "threadId", 512)?)
        .is_err()
    {
        return answer_now(id, refused(false));
    }
    let Ok(server) = text(params, "serverName", 512) else {
        record_unprojected(ELICITATION_METHOD);
        return answer_now(id, refused(true));
    };
    // The turn is Codex's correlation, not part of the request: without a
    // running Turn to hold the card nothing can ask, so it is cancelled.
    let turn = match params.get("turnId") {
        Some(Value::String(turn))
            if adapter.has_active_turn() && adapter.expect_turn(turn).is_ok() =>
        {
            turn
        }
        _ => return answer_now(id, refused(false)),
    };
    let request = match params.get("mode").and_then(Value::as_str) {
        Some("form") => params.get("requestedSchema").and_then(|schema| {
            let mut schema = schema.clone();
            strip_nulls(&mut schema);
            form::form_request(&schema)
        }),
        Some("url") => params
            .get("url")
            .and_then(Value::as_str)
            .and_then(form::url_request),
        _ => None,
    };
    let message = params
        .get("message")
        .and_then(Value::as_str)
        .map(|message| format!("{server}: {message}"));
    let asked = request.and_then(|request| {
        let summary = if request.url.is_some() {
            "Open a link for an MCP server"
        } else {
            "Answer an MCP server's questions"
        };
        let payload = form::question_payload(summary, message.as_deref(), request.clone())?;
        Some((payload, request))
    });
    // A form or link Sage cannot show exactly is declined, not guessed at.
    let Some((payload, request)) = asked else {
        record_unprojected(ELICITATION_METHOD);
        return answer_now(id, refused(true));
    };
    let item = format!("mcp-elicitation-{}", request_id_text(id)?);
    project_server_request(
        adapter,
        PendingServerRequest {
            object,
            method: ELICITATION_METHOD,
            allow_always: false,
            turn,
            item: &item,
            bytes: reply(id, "result", refused(true))?,
            payload,
            form: Some(Box::new(request)),
        },
    )
}

/// Codex re-serializes the MCP schema from typed fields, so a rule it
/// lacks may arrive as `null`; that means no rule, so it is dropped.
fn strip_nulls(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.retain(|_, value| !value.is_null());
            map.values_mut().for_each(strip_nulls);
        }
        Value::Array(values) => values.iter_mut().for_each(strip_nulls),
        _ => {}
    }
}

fn answer_now(id: &Value, result: Value) -> Result<IngressDisposition, IngressError> {
    Ok(IncomingEvent::HostToolReply {
        bytes: reply(id, "result", result)?,
    }
    .into())
}
