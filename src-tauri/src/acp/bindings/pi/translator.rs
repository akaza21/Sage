mod commands;
mod events;
mod fingerprint;
mod initialization;
mod payload;
mod responses;

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde_json::{Map, Value};

use self::fingerprint::response_fingerprint;
use self::initialization::InitializationIntent;
use self::payload::{parse_entries, parse_messages, parse_models, parse_state};
use super::{
    decode::{EXTERNAL_ID_BYTES, Record, Response, decode_record, validate_opaque_text},
    manifest::LaunchAdmission,
    protocol::{AdapterError, AdapterOutput, Operation, Outbound, TerminalOutcome, ToolOutcome},
};

const PENDING_COMMANDS: usize = 16;
const COMPLETED_COMMANDS: usize = 32;
const TOOLS_PER_TURN: usize = 512;
const UNKNOWN_KINDS_PER_TURN: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EventPhase {
    PromptIssued,
    PromptAccepted,
    AgentRunning,
    AgentEnded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingKind {
    State,
    Models,
    SetModel,
    ThinkingLevels,
    SetThinkingLevel,
    NewSession,
    Prompt,
    Abort,
    Messages,
    Entries,
}

impl PendingKind {
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
            Self::ThinkingLevels => "get_available_thinking_levels",
            Self::SetThinkingLevel => "set_thinking_level",
            Self::NewSession => "new_session",
            Self::Prompt => "prompt",
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
    event_phase: EventPhase,
    abort_requested: bool,
    authority_denied: bool,
    terminal: Option<TerminalOutcome>,
    tools: BTreeMap<String, Option<ToolOutcome>>,
    compaction_active: bool,
    thought_active: bool,
    settlement_outcome: TerminalOutcome,
    unknown_kinds: BTreeSet<String>,
    unknown_suppression_emitted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CompletedResponse {
    command: String,
    success: bool,
    payload_fingerprint: u64,
}

#[derive(Debug)]
pub struct PiTranslator {
    epoch: u64,
    next_command: u64,
    launch_admission: Option<LaunchAdmission>,
    pending: BTreeMap<String, PendingKind>,
    completed: BTreeMap<String, CompletedResponse>,
    completed_order: VecDeque<String>,
    active: Option<ActiveTurn>,
    initialization: Option<InitializationIntent>,
    pending_ui: Option<PendingUiRequest>,
    pending_thinking: Option<(String, String)>,
    closed_config_requests: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingUiRequest {
    pub id: String,
    pub method: String,
    pub options: Vec<String>,
}

impl PiTranslator {
    pub(crate) fn permission_waiting_for(&self, id: &str) -> bool {
        self.pending_ui
            .as_ref()
            .is_some_and(|request| request.id == id)
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
            Operation::NewSession => {
                if !self.pending.is_empty()
                    || self
                        .active
                        .as_ref()
                        .is_some_and(|turn| turn.terminal.is_none())
                {
                    return Err(AdapterError::SessionTransitionBusy);
                }
                self.one(PendingKind::NewSession, Map::new())
            }
            Operation::Prompt { turn_key, message } => self.begin_prompt(turn_key, message),
            Operation::Abort => {
                if self
                    .active
                    .as_ref()
                    .ok_or(AdapterError::StaleIntent)?
                    .terminal
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
                    .abort_requested = true;
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
            Operation::Resume => Err(AdapterError::UnsupportedOperation("resume")),
            Operation::Load => Err(AdapterError::UnsupportedOperation("load")),
            Operation::Close => Err(AdapterError::UnsupportedOperation("close")),
            Operation::MultiRoot => Err(AdapterError::UnsupportedOperation("multi-root")),
            Operation::ForkOrClone => Err(AdapterError::UnsupportedOperation("fork or clone")),
        }
    }

    pub fn ingest(&mut self, record: &[u8]) -> Result<Vec<AdapterOutput>, AdapterError> {
        match decode_record(record)? {
            Record::Response(response) => self.ingest_response(response),
            Record::Event { kind, value } => self.ingest_event(&kind, &value),
        }
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    pub(crate) fn model_config_pending(&self) -> bool {
        self.pending
            .iter()
            .any(|(id, kind)| kind.is_config() && !self.is_closed_config_read(id, *kind))
    }

    pub(crate) fn thinking_levels_request_id(&self) -> Option<&str> {
        self.pending.iter().find_map(|(id, kind)| {
            (*kind == PendingKind::ThinkingLevels && !self.is_closed_config_read(id, *kind))
                .then_some(id.as_str())
        })
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
