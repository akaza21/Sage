//! Claude Code's SDK protocol is stream-json over stdio: `user` messages in,
//! `assistant`/`user`/`stream_event`/`result` messages out, and
//! `control_request`/`control_response` pairs in both directions for
//! initialize, permissions, interrupts and settings.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde_json::{Value, json};

use super::config::SessionSettings;

mod commands;
mod controls;
pub(super) use controls::message_uuid;
mod inbound;
mod notices;
mod outbound;
mod permission;
mod questions;
mod runs;
mod steer;
mod subagents;
mod usage;

pub(super) use usage::UsageReply;

const MAX_PENDING: usize = 32;
/// Approval cards the session holds until the user's answer is written:
/// open, or answered but not yet delivered.
const OPEN_APPROVALS: usize = sage_core::limits::LIMITS.queues.server_requests_per_session as usize;
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

#[derive(Debug, Default, PartialEq)]
pub(super) struct Inbound {
    pub(super) acp: Vec<Value>,
    pub(super) replies: Vec<Value>,
    /// Approval cards to cancel, by wire id, before the ACP frames: Claude
    /// Code no longer waits on them.
    pub(super) withdrawn: Vec<u64>,
    pub(super) session: Vec<sage_acp_protocol::intent::session_event::SessionScopedEvent>,
    /// Questions the permission request in `acp` asks instead.
    pub(super) questions: Option<sage_acp_protocol::intent::user_input::UserInputRequest>,
    pub(super) native: Option<crate::acp::goal::NativeRpcReply>,
    pub(super) usage: Option<UsageReply>,
    /// Subagent facts, applied before the ACP frames.
    pub(super) children: Vec<crate::acp::bindings::claude::runtime::ChildEvent>,
    /// Mid-turn messages, by id: taken up (or queued to be), or not.
    pub(super) steers: Vec<(String, bool)>,
    /// Claude Code started a run of its own (after a background task ended)
    /// or asked for an approval with no turn open: the frame opens a turn,
    /// which `adopt` names, or `decline` refuses.
    pub(super) started: bool,
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
    Withdraw(String),
    Control,
    Passive(controls::PassiveRequest),
    /// A passive request whose caller gave up: its late reply is dropped.
    Abandoned,
    /// A fork, with Claude Code's profile to read the cut point from once
    /// its reply confirms the turn's transcript is written.
    Fork(Option<std::path::PathBuf>),
    ForkAt(
        Option<std::path::PathBuf>,
        sage_acp_protocol::contract::SageTurnId,
    ),
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
    /// The tool's row.
    tool: String,
    /// Claude Code's id for the subagent that asked, if one did.
    agent: Option<String>,
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
    /// Sage has prompted this session. Until then Claude Code runs nothing
    /// on its own, so nothing it sends opens a turn.
    prompted: bool,
    /// The open turn was opened for an approval alone, with no run of
    /// Claude Code's in it.
    approval_turn: bool,
    /// Sage refused the run Claude Code started: its frames are dropped
    /// until its `result`.
    declining: bool,
    /// A run of Claude Code's has started and not yet reported its `result`.
    running: bool,
    /// Claude Code's `system/init` advertises `interrupt_requeue_taken_v1`:
    /// a prompt stamped `requeue_if_interrupted_within_ms` that an interrupt
    /// stops as a run takes it in goes back to its queue instead of being lost.
    requeue: bool,
    interrupted: bool,
    auth_failed: bool,
    permissions: BTreeMap<u64, Permission>,
    /// Claude Code request ids whose answer is not yet delivered.
    answering: BTreeSet<String>,
    /// Tools the open turn's approvals allowed, with the id of the subagent
    /// that asked, if one did.
    allowed_tools: BTreeMap<String, Option<String>>,
    next_permission: u64,
    tools: BTreeMap<String, String>,
    /// Open subagents, by their `Task` tool use id.
    subagents: BTreeMap<String, subagents::Subagent>,
    /// A subagent's own tool rows: subagent and tool name, by tool use id.
    child_tools: BTreeMap<String, (String, String)>,
    streamed: VecDeque<String>,
    retrying: bool,
    compacting: bool,
    /// Mid-turn messages Claude Code has not yet taken up.
    steers: Vec<String>,
    /// The prompt Sage wrote, by its `uuid`, until Claude Code reports a
    /// run took it up: a run before that is one Claude Code started on its
    /// own, so its `result` does not end the prompt's turn.
    waiting_prompt: Option<String>,
    /// The open turn's prompt, stamped to go back to Claude Code's queue if
    /// Sage's interrupt of a declined run stops it. A stop withdraws it
    /// before interrupting, so the user's own interrupt never puts it back.
    requeuable: Option<String>,
    /// Claude Code has reported a `command_lifecycle`. A build that does not
    /// report them never says a prompt started, so no `result` is held
    /// waiting for one.
    lifecycles: bool,
    /// Why Claude Code would not run the prompt, ending the turn once the
    /// run still going reports its end.
    prompt_failure: Option<&'static str>,
    stop: steer::Stop,
    /// Requests of a stop whose turn ended unanswered, so a late reply is dropped.
    forgotten: VecDeque<String>,
    /// A run's `result` that did not end the turn, for when no waiting
    /// message runs after all.
    held: Option<Value>,
    /// What the session's context holds, from its latest main-thread model
    /// call or compaction. A turn's `result` usage sums every call of the
    /// turn, so it overstates this.
    context_tokens: Option<u64>,
    /// The model's context window, from the latest `result` that stated it.
    context_size: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Sent {
    Prompt,
    Control(String),
    Answer(String),
    Steer(String),
}

impl SdkBridge {
    pub(super) fn new(session_id: String, launch_model: Option<String>) -> Self {
        Self {
            session_id,
            launch_model,
            settings: None,
            commands: Value::Array(Vec::new()),
            pending: BTreeMap::new(),
            next_request: 0,
            prompt: None,
            prompted: false,
            approval_turn: false,
            declining: false,
            running: false,
            requeue: false,
            interrupted: false,
            auth_failed: false,
            permissions: BTreeMap::new(),
            answering: BTreeSet::new(),
            allowed_tools: BTreeMap::new(),
            next_permission: 0,
            tools: BTreeMap::new(),
            subagents: BTreeMap::new(),
            child_tools: BTreeMap::new(),
            streamed: VecDeque::new(),
            retrying: false,
            compacting: false,
            steers: Vec::new(),
            waiting_prompt: None,
            requeuable: None,
            lifecycles: false,
            prompt_failure: None,
            stop: steer::Stop::default(),
            forgotten: VecDeque::new(),
            held: None,
            context_tokens: None,
            context_size: None,
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

    pub(super) fn undo(&mut self, sent: &Sent) {
        match sent {
            Sent::Prompt => {
                self.prompt = None;
                self.waiting_prompt = None;
                self.requeuable = None;
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

#[cfg(test)]
mod subagent_tests;

#[cfg(test)]
mod harness_turn_tests;
