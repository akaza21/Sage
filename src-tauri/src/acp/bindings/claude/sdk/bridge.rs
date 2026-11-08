//! The Agent SDK protocol, presented as ACP session traffic.
//!
//! Claude Code's SDK protocol is stream-json over stdio: `user` messages in,
//! `assistant`/`user`/`stream_event`/`result` messages out, and
//! `control_request`/`control_response` pairs in both directions for
//! initialize, permissions, interrupts and settings. Sage's Claude session
//! state machine and translator speak ACP, the model every binding projects
//! into. The bridge holds the small amount of state needed to map one onto
//! the other: which control reply answers which ACP request, which ACP
//! permission id stands for which `can_use_tool` request, and the running
//! prompt that a `result` completes.

use std::collections::{BTreeMap, VecDeque};

use serde_json::{Value, json};

use super::config::SessionSettings;

mod inbound;
mod outbound;
mod permission;

/// Bounds on per-turn bookkeeping; the inner session caps pending requests.
const MAX_PENDING: usize = 32;
const MAX_TRACKED_TOOLS: usize = 1_024;
/// A message's final frames follow its stream directly, so only the most
/// recent streamed ids are ever looked up.
const RECENT_STREAMED_MESSAGES: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(super) enum BridgeError {
    #[error("Claude Code sent a frame Sage cannot map")]
    Frame,
    #[error("the session sent a request Claude Code has no equivalent for")]
    Unsupported,
    #[error("too many Claude Code requests are pending")]
    Capacity,
}

/// What one SDK frame becomes: ACP frames for the session, and replies Sage
/// owes Claude Code directly (for control requests the session never sees).
#[derive(Debug, Default, PartialEq)]
pub(super) struct Inbound {
    pub(super) acp: Vec<Value>,
    pub(super) replies: Vec<Value>,
}

#[derive(Debug)]
enum Pending {
    Initialize(Value),
    Session(Value),
    Mode {
        id: Value,
        mode: String,
    },
    Option {
        id: Value,
        config_id: String,
        value: String,
    },
    Interrupt,
}

#[derive(Debug)]
struct Permission {
    request_id: String,
    input: Value,
    suggestions: Option<Value>,
}

#[derive(Debug)]
pub(super) struct SdkBridge {
    session_id: String,
    launch_model: Option<String>,
    settings: Option<SessionSettings>,
    commands: Value,
    pending: BTreeMap<String, Pending>,
    next_request: u64,
    prompt: Option<Value>,
    interrupted: bool,
    auth_failed: bool,
    permissions: BTreeMap<u64, Permission>,
    /// Withdrawn or outlived requests: wire id to Claude Code request id.
    settled: BTreeMap<u64, String>,
    next_permission: u64,
    tools: BTreeMap<String, String>,
    streamed: VecDeque<String>,
}

/// What a written SDK frame changed, so an undelivered one can be undone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Sent {
    Prompt,
    Control(String),
}

impl SdkBridge {
    /// `session_id` is the Claude session the CLI was launched to create or
    /// resume; `launch_model` the model it was launched with, if any.
    pub(super) fn new(session_id: String, launch_model: Option<String>) -> Self {
        Self {
            session_id,
            launch_model,
            settings: None,
            commands: Value::Array(Vec::new()),
            pending: BTreeMap::new(),
            next_request: 0,
            prompt: None,
            interrupted: false,
            auth_failed: false,
            permissions: BTreeMap::new(),
            settled: BTreeMap::new(),
            next_permission: 0,
            tools: BTreeMap::new(),
            streamed: VecDeque::new(),
        }
    }

    fn control(&mut self, request: Value, pending: Pending) -> Result<Value, BridgeError> {
        if self.pending.len() >= MAX_PENDING {
            return Err(BridgeError::Capacity);
        }
        self.next_request += 1;
        let request_id = format!("sage-{}", self.next_request);
        self.pending.insert(request_id.clone(), pending);
        Ok(json!({"type": "control_request", "request_id": request_id, "request": request}))
    }

    /// The state a written frame changed, if Claude Code never receiving it
    /// would leave the bridge waiting.
    pub(super) fn sent(sdk: &Value) -> Option<Sent> {
        match sdk.get("type").and_then(Value::as_str)? {
            "user" => Some(Sent::Prompt),
            "control_request" => Some(Sent::Control(sdk.get("request_id")?.as_str()?.to_owned())),
            _ => None,
        }
    }

    /// Claude Code never received a frame: forget what it started.
    pub(super) fn undo(&mut self, sent: &Sent) {
        match sent {
            Sent::Prompt => {
                self.prompt = None;
                self.interrupted = false;
            }
            Sent::Control(request_id) => {
                self.pending.remove(request_id);
            }
        }
    }

    fn update(&self, update: Value) -> Value {
        json!({
            "jsonrpc": "2.0",
            "method": "session/update",
            "params": {"sessionId": self.session_id, "update": update},
        })
    }
}

fn acp_result(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn acp_error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

#[cfg(test)]
mod tests;
