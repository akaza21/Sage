use super::{
    BrowserCapability, BrowserFailure, BrowserRequest, BrowserResponse, BrowserScope,
    admit_request_json, dispatch,
};
use crate::acp::limits::BROWSER_LIMITS;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::io::{BufRead, Write};

mod auth;
mod schema;
use schema::tools;

pub struct BrowserMcpToken(String);

pub struct BrowserMcpServer<P: BrowserCapability> {
    port: P,
    session_id: String,
    fixed_scope: Option<BrowserScope>,
    token_digest: [u8; 32],
    reconnect_digest: Option<[u8; 32]>,
    expires_at: Option<u64>,
    connected: bool,
    accepted_connections: u8,
    operation_observer: Option<Box<dyn BrowserOperationObserver + Send>>,
}

pub trait BrowserOperationObserver {
    fn started(&mut self, request: &BrowserRequest) -> Result<(), BrowserMcpError>;
    fn finished(
        &mut self,
        request: &BrowserRequest,
        outcome: &Result<BrowserResponse, BrowserFailure>,
    ) -> Result<(), BrowserMcpError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserMcpError {
    Unauthorized,
    Expired,
    InvalidRequest,
    InvalidActionShape,
    Capacity,
    Unavailable,
    OutcomeUnknown,
}

pub(crate) const MAX_SESSION_CONNECTIONS: u8 = 4;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    jsonrpc: String,
    id: Value,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CallParams {
    name: String,
    arguments: Value,
    #[serde(default, rename = "_meta")]
    _meta: Option<Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ToolScope {
    #[serde(default)]
    tab_id: Option<u64>,
    #[serde(default)]
    session_id: Option<String>,
}

impl<P: BrowserCapability> BrowserMcpServer<P> {
    pub fn issue(port: P, scope: BrowserScope, now_ms: u64) -> (Self, BrowserMcpToken) {
        Self::issue_inner(port, scope.session_id.clone(), Some(scope), now_ms)
    }

    pub fn issue_for_session(port: P, session_id: String, now_ms: u64) -> (Self, BrowserMcpToken) {
        Self::issue_inner(port, session_id, None, now_ms)
    }

    fn issue_inner(
        port: P,
        session_id: String,
        fixed_scope: Option<BrowserScope>,
        now_ms: u64,
    ) -> (Self, BrowserMcpToken) {
        let token = BrowserMcpToken(format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        ));
        let token_digest = Sha256::digest(token.0.as_bytes()).into();
        let expires_at = fixed_scope.as_ref().map(|_| now_ms.saturating_add(60_000));
        let reconnect_digest = fixed_scope.is_none().then_some(token_digest);
        (
            Self {
                port,
                session_id,
                fixed_scope,
                token_digest,
                reconnect_digest,
                expires_at,
                connected: false,
                accepted_connections: 0,
                operation_observer: None,
            },
            token,
        )
    }

    pub fn set_operation_observer(&mut self, observer: Box<dyn BrowserOperationObserver + Send>) {
        self.operation_observer = Some(observer);
    }

    pub fn handle(&mut self, frame: &[u8]) -> Result<Vec<u8>, BrowserMcpError> {
        if !self.connected {
            return Err(BrowserMcpError::Unauthorized);
        }
        if frame.len() > BROWSER_LIMITS.request_bytes {
            return Err(BrowserMcpError::Capacity);
        }
        let value: Value =
            serde_json::from_slice(frame).map_err(|_| BrowserMcpError::InvalidRequest)?;
        if value["jsonrpc"] == "2.0"
            && value.get("id").is_none()
            && value.get("method").and_then(Value::as_str).is_some()
        {
            // Calls are served serially on this pipe, so a cancellation frame
            // can only arrive after its request has finished. Unknown and
            // already-terminal notifications never require a response.
            return Ok(Vec::new());
        }
        let envelope: Envelope =
            serde_json::from_value(value).map_err(|_| BrowserMcpError::InvalidRequest)?;
        if envelope.jsonrpc != "2.0" || (!envelope.id.is_string() && !envelope.id.is_number()) {
            return Err(BrowserMcpError::InvalidRequest);
        }
        let result = match envelope.method.as_str() {
            "initialize" => {
                if !envelope.params.is_object() {
                    return json_rpc_error(&envelope.id, -32602, "Invalid params");
                }
                json!({
                    "protocolVersion":"2025-06-18","capabilities":{"tools":{}},
                    "serverInfo":{"name":"sage-browser","version":env!("CARGO_PKG_VERSION")}
                })
            }
            "tools/list" => {
                if !valid_tools_list_params(&envelope.params) {
                    return json_rpc_error(&envelope.id, -32602, "Invalid params");
                }
                json!({"tools":tools()})
            }
            "tools/call" => match self.call(envelope.params) {
                Ok(result) => result,
                Err(BrowserMcpError::InvalidRequest) => {
                    return json_rpc_error(&envelope.id, -32602, "Invalid params");
                }
                Err(BrowserMcpError::InvalidActionShape) => {
                    return json_rpc_error(
                        &envelope.id,
                        -32602,
                        "Invalid params: action.kind is required; use action.kind, action.handle, and action.value directly for fill",
                    );
                }
                Err(BrowserMcpError::OutcomeUnknown) => json!({
                    "content":[{"type":"text","text":"The browser operation may have been issued, but its outcome is unknown. Do not retry automatically; inspect the page state first."}],
                    "isError":true
                }),
                Err(BrowserMcpError::Unavailable) => json!({
                    "content":[{"type":"text","text":"The browser operation failed or is unavailable. Inspect the page state before retrying."}],
                    "isError":true
                }),
                Err(BrowserMcpError::Capacity) => json!({
                    "content":[{"type":"text","text":"The browser operation exceeded a configured limit."}],
                    "isError":true
                }),
                Err(BrowserMcpError::Unauthorized) => json!({
                    "content":[{"type":"text","text":"Browser scope is not authorized for this session."}],
                    "isError":true
                }),
                Err(error) => return Err(error),
            },
            _ => return json_rpc_error(&envelope.id, -32601, "Method not found"),
        };
        let response =
            serde_json::to_vec(&json!({"jsonrpc":"2.0","id":envelope.id,"result":result}))
                .map_err(|_| BrowserMcpError::Unavailable)?;
        if response.len() > BROWSER_LIMITS.result_bytes {
            return Err(BrowserMcpError::Capacity);
        }
        Ok(response)
    }

    fn call(&mut self, params: Value) -> Result<Value, BrowserMcpError> {
        let call: CallParams =
            serde_json::from_value(params).map_err(|_| BrowserMcpError::InvalidRequest)?;
        let kind = match call.name.as_str() {
            "openTab" | "navigate" | "snapshotPage" | "act" | "waitFor" | "screenshot"
            | "readConsole" | "readNetwork" => call.name,
            _ => return Err(BrowserMcpError::InvalidRequest),
        };
        let mut arguments = call
            .arguments
            .as_object()
            .cloned()
            .ok_or(BrowserMcpError::InvalidRequest)?;
        let supplied_scope: ToolScope =
            serde_json::from_value(arguments.remove("scope").unwrap_or_else(|| json!({})))
                .map_err(|_| BrowserMcpError::InvalidRequest)?;
        if supplied_scope
            .session_id
            .as_deref()
            .is_some_and(|supplied| supplied != self.session_id)
        {
            return Err(BrowserMcpError::Unauthorized);
        }
        let tab_id = if kind == "openTab" {
            if supplied_scope.tab_id.is_some() {
                return Err(BrowserMcpError::InvalidRequest);
            }
            0
        } else {
            let Some(tab_id) = supplied_scope
                .tab_id
                .or_else(|| self.fixed_scope.as_ref().map(|scope| scope.tab_id))
                .or_else(|| self.port.attached_tab_id())
            else {
                return Ok(
                    json!({"content":[{"type":"text","text":"No browser tab attached; call openTab with an HTTP(S) URL, or ask the user to attach one."}],"isError":true}),
                );
            };
            tab_id
        };
        let scope = BrowserScope {
            session_id: self.session_id.clone(),
            tab_id,
        };
        if self
            .fixed_scope
            .as_ref()
            .is_some_and(|fixed| fixed != &scope)
        {
            return Err(BrowserMcpError::Unauthorized);
        }
        if arguments.contains_key("kind") {
            return Err(BrowserMcpError::InvalidRequest);
        }
        if kind == "act"
            && arguments
                .get("action")
                .and_then(Value::as_object)
                .is_some_and(|action| !action.contains_key("kind"))
        {
            return Err(BrowserMcpError::InvalidActionShape);
        }
        arguments.insert("kind".into(), Value::String(kind));
        let bytes = serde_json::to_vec(&json!({"scope":scope,"command":arguments}))
            .map_err(|_| BrowserMcpError::InvalidRequest)?;
        let request = admit_request_json(&bytes).map_err(|failure| match failure {
            BrowserFailure::Capacity => BrowserMcpError::Capacity,
            _ => BrowserMcpError::InvalidRequest,
        })?;
        if let Some(observer) = self.operation_observer.as_mut() {
            observer.started(&request)?;
        }
        let outcome = dispatch(&mut self.port, request.clone());
        if let Some(observer) = self.operation_observer.as_mut() {
            observer.finished(&request, &outcome)?;
        }
        let response = match outcome {
            Err(super::BrowserFailure::NoAttachment) => return Ok(json!({"content":[{"type":"text","text":"No browser tab attached; call openTab with an HTTP(S) URL, or ask the user to attach one."}],"isError":true})),
            Err(super::BrowserFailure::UserControl) => return Ok(json!({"content":[{"type":"text","text":"The user took over this browser tab and has not handed it back. Do not retry now; tell the user you will continue once they hand the tab back."}],"isError":true})),
            Err(super::BrowserFailure::OutcomeUnknown) => return Ok(json!({"content":[{"type":"text","text":"The browser operation was issued, but its outcome is unknown. Do not retry automatically; inspect the page state first."}],"isError":true})),
            other => other,
        }.map_err(|failure| match failure {
            super::BrowserFailure::Capacity => BrowserMcpError::Capacity,
            super::BrowserFailure::Denied => BrowserMcpError::Unauthorized,
            super::BrowserFailure::InvalidRequest | super::BrowserFailure::StaleHandle => {
                BrowserMcpError::InvalidRequest
            }
            super::BrowserFailure::Unavailable => BrowserMcpError::Unavailable,
            super::BrowserFailure::OutcomeUnknown => BrowserMcpError::OutcomeUnknown,
            super::BrowserFailure::NoAttachment | super::BrowserFailure::UserControl => {
                BrowserMcpError::Unavailable
            }
        })?;
        let text = serde_json::to_string(&response).map_err(|_| BrowserMcpError::Unavailable)?;
        Ok(json!({"content":[{"type":"text","text":text}],"isError":false}))
    }

    pub fn serve_ndjson<R: BufRead, W: Write>(
        &mut self,
        reader: &mut R,
        writer: &mut W,
    ) -> Result<(), BrowserMcpError> {
        if !self.connected {
            return Err(BrowserMcpError::Unauthorized);
        }
        let mut frame = Vec::new();
        while read_bounded_line(reader, &mut frame)? {
            let mut response = self.handle(&frame)?;
            if response.is_empty() {
                frame.clear();
                continue;
            }
            response.push(b'\n');
            writer
                .write_all(&response)
                .map_err(|_| BrowserMcpError::Unavailable)?;
            frame.clear();
        }
        Ok(())
    }
}

fn valid_tools_list_params(params: &Value) -> bool {
    let Some(object) = params.as_object() else {
        return params.is_null();
    };
    object.iter().all(|(name, value)| match name.as_str() {
        "cursor" => value.is_null(),
        "_meta" => value.is_object(),
        _ => false,
    })
}

fn json_rpc_error(id: &Value, code: i32, message: &str) -> Result<Vec<u8>, BrowserMcpError> {
    serde_json::to_vec(&json!({
        "jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}
    }))
    .map_err(|_| BrowserMcpError::Unavailable)
}

fn read_bounded_line(
    reader: &mut impl BufRead,
    frame: &mut Vec<u8>,
) -> Result<bool, BrowserMcpError> {
    loop {
        let buffer = reader
            .fill_buf()
            .map_err(|_| BrowserMcpError::Unavailable)?;
        if buffer.is_empty() {
            return Ok(!frame.is_empty());
        }
        let take = buffer
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(buffer.len(), |index| index + 1);
        if frame.len().saturating_add(take) > BROWSER_LIMITS.request_bytes {
            return Err(BrowserMcpError::Capacity);
        }
        let done = buffer[take - 1] == b'\n';
        frame.extend_from_slice(&buffer[..take]);
        reader.consume(take);
        if done {
            return Ok(true);
        }
    }
}

#[cfg(test)]
mod tests;
pub mod transport;
