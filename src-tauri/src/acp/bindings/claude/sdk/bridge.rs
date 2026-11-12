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

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde_json::{Value, json};

use super::config::SessionSettings;

mod commands;
mod controls;
mod inbound;
mod notices;
mod outbound;
mod permission;
mod questions;
mod steer;
mod subagents;

/// Bounds on per-turn bookkeeping; the inner session caps pending requests.
const MAX_PENDING: usize = 32;
/// Approval cards the session holds until the user's answer is written:
/// open, or answered but not yet delivered.
const OPEN_APPROVALS: usize = crate::acp::limits::LIMITS
    .queues
    .server_requests_per_session as usize;
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
    /// Approval cards to cancel, by wire id, before the ACP frames: Claude
    /// Code no longer waits on them.
    pub(super) withdrawn: Vec<u64>,
    /// Session-scoped facts: notices, activities, the effective model.
    pub(super) session: Vec<crate::acp::intent::session_event::SessionScopedEvent>,
    /// Questions the permission request in `acp` asks instead.
    pub(super) questions: Option<crate::acp::intent::user_input::UserInputRequest>,
    /// The reply to a session control.
    pub(super) native: Option<crate::acp::goal::NativeRpcReply>,
    /// Subagent facts, applied before the ACP frames.
    pub(super) children: Vec<crate::acp::bindings::claude::runtime::ChildEvent>,
    /// Mid-turn messages, by id: taken up (or queued to be), or not.
    pub(super) steers: Vec<(String, bool)>,
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
    /// Withdrawing a waiting mid-turn message, on the way to an interrupt.
    Withdraw(String),
    /// A session control; its reply completes the native request.
    Control,
    /// An MCP request Sage stops waiting for at its deadline.
    Mcp(controls::McpRequest),
    /// An MCP request whose caller gave up: its late reply is dropped.
    Abandoned,
    /// A fork, with Claude Code's profile to read the cut point from once
    /// its reply confirms the turn's transcript is written.
    Fork(Option<std::path::PathBuf>),
}

#[derive(Debug)]
struct Permission {
    request_id: String,
    input: Value,
    suggestions: Option<Value>,
    /// An `AskUserQuestion` card: its questions' texts, in order.
    questions: Option<Vec<String>>,
    /// An `ExitPlanMode` card: declining it ends the turn in plan mode.
    plan_exit: bool,
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
    /// Claude Code request ids whose answer is not yet delivered.
    answering: BTreeSet<String>,
    next_permission: u64,
    tools: BTreeMap<String, String>,
    /// Open subagents, by their `Task` tool use id.
    subagents: BTreeSet<String>,
    /// A subagent's own tool rows: subagent and tool name, by tool use id.
    child_tools: BTreeMap<String, (String, String)>,
    streamed: VecDeque<String>,
    retrying: bool,
    compacting: bool,
    /// Mid-turn messages Claude Code has not yet taken up.
    steers: Vec<String>,
    /// A run's `result` that did not end the turn, for when no waiting
    /// message runs after all.
    held: Option<Value>,
}

/// What a written SDK frame changed, so an undelivered one can be undone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Sent {
    Prompt,
    Control(String),
    Answer(String),
    /// A mid-turn message, by id.
    Steer(String),
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
            answering: BTreeSet::new(),
            next_permission: 0,
            tools: BTreeMap::new(),
            subagents: BTreeSet::new(),
            child_tools: BTreeMap::new(),
            streamed: VecDeque::new(),
            retrying: false,
            compacting: false,
            steers: Vec::new(),
            held: None,
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
            "control_response" => Some(Sent::Answer(
                sdk.get("response")?.get("request_id")?.as_str()?.to_owned(),
            )),
            _ => None,
        }
    }

    /// The error reply to a control request Sage could not read, so Claude
    /// Code does not wait on it.
    pub(super) fn refusal(sdk: &Value) -> Option<Value> {
        (sdk.get("type")?.as_str()? == "control_request").then_some(())?;
        let request_id = sdk.get("request_id")?.as_str()?;
        Some(json!({
            "type": "control_response",
            "response": {"subtype": "error", "request_id": request_id, "error": "Sage could not read this request"},
        }))
    }

    /// What Sage still needs from a frame too large to read: a turn's end,
    /// or a failed reply to one of Sage's own requests.
    pub(super) fn essentials(sdk: &Value) -> Option<Value> {
        match sdk.get("type")?.as_str()? {
            "result" => {
                let mut kept = json!({"type": "result", "usage": {}});
                for key in ["subtype", "is_error", "stop_reason", "session_id"] {
                    if let Some(value) = sdk.get(key).filter(|value| !value.is_object()) {
                        kept[key] = value.clone();
                    }
                }
                let usage = sdk.get("usage").and_then(Value::as_object);
                for (key, count) in usage.into_iter().flatten() {
                    if count.is_u64() {
                        kept["usage"][key] = count.clone();
                    }
                }
                Some(kept)
            }
            "control_response" => Some(json!({
                "type": "control_response",
                "response": {
                    "subtype": "error",
                    "request_id": sdk.get("response")?.get("request_id")?.as_str()?,
                    "error": "Claude Code's reply was too large for Sage",
                },
            })),
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
                if let Some(Pending::Interrupt | Pending::Withdraw(_)) =
                    self.pending.remove(request_id)
                {
                    self.interrupted = false;
                }
            }
            Sent::Answer(_) => self.delivered(sent),
            Sent::Steer(uuid) => self.unsteer(uuid),
        }
    }

    /// A frame reached Claude Code (or never will): an answered approval no
    /// longer holds the session's card.
    pub(super) fn delivered(&mut self, sent: &Sent) {
        if let Sent::Answer(request_id) = sent {
            self.answering.remove(request_id);
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
