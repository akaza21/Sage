mod commands;
mod events;
mod fingerprint;
mod initialization;
mod lifecycle;
mod payload;
mod responses;

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde_json::{Map, Value};

use crate::acp::intent::{PermissionOptionKind, user_input::UserInputRequest};

use self::fingerprint::response_fingerprint;
use self::initialization::InitializationIntent;
use self::lifecycle::{TurnLifecycle, TurnPhase};
use self::payload::{parse_entries, parse_messages, parse_models, parse_state};
use super::{
    decode::{EXTERNAL_ID_BYTES, Record, Response, decode_record, validate_opaque_text},
    manifest::LaunchAdmission,
    protocol::{
        AdapterError, AdapterOutput, Operation, Outbound, TerminalOutcome, ToolOutcome,
        is_allow_option,
    },
};

const PENDING_COMMANDS: usize = 16;
const COMPLETED_COMMANDS: usize = 32;
const TOOLS_PER_TURN: usize = 512;
const UNKNOWN_KINDS_PER_TURN: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingKind {
    State,
    Models,
    SetModel,
    ThinkingLevels,
    Commands,
    SetThinkingLevel,
    NewSession,
    Resume,
    Prompt,
    Compact,
    Steer,
    ClearQueue,
    Abort,
    Messages,
    Entries,
}

impl PendingKind {
    /// Requests that open a Turn the user sees.
    const fn starts_turn(self) -> bool {
        matches!(self, Self::Prompt | Self::Compact)
    }

    const fn is_config(self) -> bool {
        matches!(
            self,
            Self::State | Self::SetModel | Self::ThinkingLevels | Self::SetThinkingLevel
        )
    }

    const fn command(self) -> &'static str {
        match self {
            Self::State => "get_state",
            Self::Models => "get_available_models",
            Self::SetModel => "set_model",
            Self::Commands => "get_commands",
            Self::ThinkingLevels => "get_available_thinking_levels",
            Self::SetThinkingLevel => "set_thinking_level",
            // Pi opens a fresh session at launch and Sage runs one session
            // per process, so a new chat takes that session by its id.
            Self::NewSession => "get_state",
            Self::Resume => "switch_session",
            Self::Prompt => "prompt",
            Self::Compact => "compact",
            Self::Steer => "steer",
            Self::ClearQueue => "clear_queue",
            Self::Abort => "abort",
            Self::Messages => "get_messages",
            Self::Entries => "get_entries",
        }
    }
}

#[derive(Debug)]
struct ActiveTurn {
    turn_key: String,
    prompt_command_id: String,
    lifecycle: TurnLifecycle,
    authority_denied: bool,
    tools: BTreeMap<String, Option<ToolOutcome>>,
    compaction_active: bool,
    /// Pi's reason for a failed compaction has reached the user.
    compaction_failure_shown: bool,
    thought_active: bool,
    settlement_outcome: TerminalOutcome,
    settlement_message: Option<String>,
    retry_attempt: Option<u64>,
    retry_notices: u8,
    retry_failed: bool,
    retry_final_error: Option<String>,
    unknown_kinds: BTreeSet<String>,
    unknown_suppression_emitted: bool,
    /// The last assistant reply's stop reason, error and usage, kept from
    /// its `message_end` in case the run's `agent_end` is too large to read.
    last_reply: Option<Box<Value>>,
    /// An oversized event of this run was dropped unread.
    dropped_frame: bool,
    /// An assistant reply has started and not yet ended.
    reply_open: bool,
}

#[derive(Debug, Clone, Copy)]
struct PendingRequest {
    kind: PendingKind,
    // Ownership survives settlement and replacement of the active turn, within
    // the same 16-entry pending bound as every other command.
    owner: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CompletedResponse {
    command: String,
    success: bool,
    payload_fingerprint: u64,
}

#[derive(Debug)]
pub struct PiTranslator {
    pub(crate) bridge_tools: bool,
    epoch: u64,
    next_command: u64,
    launch_admission: Option<LaunchAdmission>,
    pending: BTreeMap<String, PendingRequest>,
    completed: BTreeMap<String, CompletedResponse>,
    completed_order: VecDeque<String>,
    active: Option<ActiveTurn>,
    initialization: Option<InitializationIntent>,
    pending_ui: Option<PendingUiRequest>,
    pending_thinking: Option<(String, String)>,
    resuming: Option<String>,
    closed_config_requests: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingUiRequest {
    pub id: String,
    pub method: String,
    pub options: Vec<String>,
    /// An extension question shown on the shared question card.
    pub form: Option<Box<UserInputRequest>>,
    /// Pi drops a dialog with a timeout on its own, without telling Sage.
    pub expires: bool,
}

impl PendingUiRequest {
    /// The answers Sage can deliver for this dialog: a single approval where
    /// the dialog has an allow answer, and always a denial. Pi has no standing
    /// approval, so "always allow" is never offered.
    pub(crate) fn offered_option_kinds(&self) -> Vec<PermissionOptionKind> {
        use PermissionOptionKind::{AllowOnce, RejectOnce};
        let allows = match self.method.as_str() {
            BRIDGE_WRITE_METHOD | "confirm" => true,
            "select" => self.options.iter().any(|option| is_allow_option(option)),
            _ => false,
        };
        if allows {
            vec![AllowOnce, RejectOnce]
        } else {
            vec![RejectOnce]
        }
    }
}

/// The field an extension question's answer is read from.
pub(crate) const ANSWER_ID: &str = "answer";

/// The pending dialog of a bridged workspace edit awaiting the user's decision.
pub(crate) const BRIDGE_WRITE_METHOD: &str = "sage-write";

impl PiTranslator {
    /// Whether an answer may still arrive that could outgrow a frame: any
    /// read, as opposed to the turn commands Pi answers in a few bytes.
    pub(crate) fn awaits_large_response(&self) -> bool {
        self.pending.values().any(|request| {
            !matches!(
                request.kind,
                PendingKind::Prompt
                    | PendingKind::Compact
                    | PendingKind::Steer
                    | PendingKind::ClearQueue
                    | PendingKind::Abort
                    | PendingKind::SetModel
                    | PendingKind::SetThinkingLevel
            )
        })
    }

    pub(crate) fn permission_waiting_for(&self, id: &str) -> bool {
        self.pending_ui
            .as_ref()
            .is_some_and(|request| request.id == id)
    }

    pub(crate) fn pending_ui(&self) -> Option<&PendingUiRequest> {
        self.pending_ui.as_ref()
    }

    pub(crate) fn take_pending_ui(&mut self) -> Option<PendingUiRequest> {
        self.pending_ui.take()
    }

    pub(crate) fn remember_pending_ui(&mut self, request: PendingUiRequest) {
        self.pending_ui = Some(request);
    }

    fn admitted(epoch: u64, admission: LaunchAdmission) -> Self {
        debug_assert!(admission.capabilities().prompt());
        debug_assert!(!admission.capabilities().write_tools());
        Self::new(epoch, Some(admission))
    }

    pub(crate) fn native_admitted(epoch: u64) -> Self {
        Self::admitted(epoch, LaunchAdmission::read_only())
    }

    #[cfg(any(test, all(windows, feature = "acp-fixture", debug_assertions)))]
    pub(crate) fn fixture_admitted(epoch: u64) -> Self {
        Self::admitted(epoch, LaunchAdmission::fixture())
    }

    #[cfg(test)]
    pub(crate) fn unavailable(epoch: u64) -> Self {
        Self::new(epoch, None)
    }

    fn new(epoch: u64, launch_admission: Option<LaunchAdmission>) -> Self {
        Self {
            bridge_tools: false,
            epoch,
            next_command: 0,
            launch_admission,
            pending: BTreeMap::new(),
            completed: BTreeMap::new(),
            completed_order: VecDeque::new(),
            active: None,
            initialization: None,
            pending_ui: None,
            pending_thinking: None,
            resuming: None,
            closed_config_requests: BTreeSet::new(),
        }
    }

    pub fn begin(&mut self, operation: Operation) -> Result<Vec<Outbound>, AdapterError> {
        match operation {
            Operation::Initialize => self.begin_initialize(),
            Operation::GetModels => self.one(PendingKind::Models, Map::new()),
            Operation::SetModel { provider, id } => {
                validate_id("provider", &provider)?;
                validate_id("model ID", &id)?;
                let mut fields = Map::new();
                fields.insert("provider".into(), Value::String(provider));
                fields.insert("modelId".into(), Value::String(id));
                self.one(PendingKind::SetModel, fields)
            }
            Operation::GetCommands => self.one(PendingKind::Commands, Map::new()),
            Operation::GetThinkingLevels => self.one(PendingKind::ThinkingLevels, Map::new()),
            Operation::SetThinkingLevel { level } => {
                if self.pending_thinking.is_some() || !valid_thinking_level(&level) {
                    return Err(AdapterError::Malformed);
                }
                let mut fields = Map::new();
                fields.insert("level".into(), Value::String(level.clone()));
                let outbound = self.issue(PendingKind::SetThinkingLevel, fields)?;
                self.pending_thinking = Some((outbound.command_id.clone(), level));
                Ok(vec![outbound])
            }
            Operation::NewSession | Operation::Resume { .. } => {
                if !self.pending.is_empty()
                    || self
                        .active
                        .as_ref()
                        .is_some_and(|turn| turn.lifecycle.terminal().is_none())
                {
                    return Err(AdapterError::SessionTransitionBusy);
                }
                let Operation::Resume { session_id, path } = operation else {
                    return self.one(PendingKind::NewSession, Map::new());
                };
                validate_id("session ID", &session_id)?;
                let mut fields = Map::new();
                fields.insert("sessionPath".into(), Value::String(path));
                let outbound = self.one(PendingKind::Resume, fields)?;
                self.resuming = Some(session_id);
                Ok(outbound)
            }
            Operation::Prompt { turn_key, message } => self.begin_prompt(turn_key, message),
            Operation::Compact {
                turn_key,
                instructions,
            } => self.begin_compact(turn_key, instructions),
            Operation::Steer { message } => self.begin_steer(message),
            Operation::ClearQueue => self.one(PendingKind::ClearQueue, Map::new()),
            Operation::Abort => {
                if self
                    .active
                    .as_ref()
                    .ok_or(AdapterError::StaleIntent)?
                    .lifecycle
                    .terminal()
                    .is_some()
                {
                    return Err(AdapterError::StaleIntent);
                }
                // A failed enqueue must not clear a prior permission-denial
                // latch; `issue` mutates no Turn state on capacity failure.
                let outbound = self.one(PendingKind::Abort, Map::new())?;
                self.active
                    .as_mut()
                    .ok_or(AdapterError::StaleIntent)?
                    .lifecycle
                    .request_abort();
                Ok(outbound)
            }
            Operation::GetState => self.one(PendingKind::State, Map::new()),
            Operation::GetMessages => self.one(PendingKind::Messages, Map::new()),
            Operation::GetEntries { since } => {
                let mut fields = Map::new();
                if let Some(since) = since {
                    validate_id("entry cursor", &since)?;
                    fields.insert("since".into(), Value::String(since));
                }
                self.one(PendingKind::Entries, fields)
            }
            Operation::Load => Err(AdapterError::UnsupportedOperation("load")),
            Operation::Close => Err(AdapterError::UnsupportedOperation("close")),
            Operation::MultiRoot => Err(AdapterError::UnsupportedOperation("multi-root")),
            Operation::ForkOrClone => Err(AdapterError::UnsupportedOperation("fork or clone")),
        }
    }

    pub fn ingest(&mut self, record: &[u8]) -> Result<Vec<AdapterOutput>, AdapterError> {
        self.ingest_record(decode_record(record)?)
    }

    pub(super) fn ingest_record(
        &mut self,
        record: Record,
    ) -> Result<Vec<AdapterOutput>, AdapterError> {
        match record {
            Record::Response(response) => self.ingest_response(response),
            Record::Event { kind, value } => self.ingest_event(&kind, &value),
        }
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    pub(crate) fn model_config_pending(&self) -> bool {
        self.pending.iter().any(|(id, request)| {
            request.kind.is_config() && !self.is_closed_config_read(id, request.kind)
        })
    }

    pub(crate) fn thinking_levels_request_id(&self) -> Option<&str> {
        self.pending.iter().find_map(|(id, request)| {
            (request.kind == PendingKind::ThinkingLevels
                && !self.is_closed_config_read(id, request.kind))
            .then_some(id.as_str())
        })
    }

    pub(crate) fn sole_optional_discovery_request_id(&self, command: &str) -> Option<&str> {
        let kind = match command {
            "get_available_thinking_levels" => PendingKind::ThinkingLevels,
            "get_commands" => PendingKind::Commands,
            _ => return None,
        };
        let mut requests = self
            .pending
            .iter()
            .filter(|(_, request)| request.kind == kind);
        let (id, _) = requests.next()?;
        requests.next().is_none().then_some(id.as_str())
    }

    fn is_closed_config_read(&self, id: &str, kind: PendingKind) -> bool {
        matches!(kind, PendingKind::State | PendingKind::ThinkingLevels)
            && self.closed_config_requests.contains(id)
    }
}

fn validate_id(field: &'static str, value: &str) -> Result<(), AdapterError> {
    validate_opaque_text(value, EXTERNAL_ID_BYTES).map_err(|_| AdapterError::Limit(field))
}

fn valid_thinking_level(level: &str) -> bool {
    matches!(
        level,
        "off" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max"
    )
}
