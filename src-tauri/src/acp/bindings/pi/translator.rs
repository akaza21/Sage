mod events;
mod fingerprint;
mod initialization;
mod payload;

use std::collections::{BTreeMap, VecDeque};

use serde_json::{Map, Value};

use self::fingerprint::response_fingerprint;
use self::initialization::InitializationIntent;
use self::payload::{parse_entries, parse_messages, parse_models, parse_state};
use super::{
    decode::{
        EXTERNAL_ID_BYTES, PROMPT_BYTES, Record, Response, decode_record, validate_opaque_text,
    },
    manifest::LaunchAdmission,
    protocol::{AdapterError, AdapterOutput, Operation, Outbound, TerminalOutcome, ToolOutcome},
};

const PENDING_COMMANDS: usize = 16;
const COMPLETED_COMMANDS: usize = 32;
const TOOLS_PER_TURN: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EventPhase {
    PromptIssued,
    PromptAccepted,
    AgentRunning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingKind {
    State,
    Models,
    NewSession,
    Prompt,
    Abort,
    Messages,
    Entries,
}

impl PendingKind {
    const fn command(self) -> &'static str {
        match self {
            Self::State => "get_state",
            Self::Models => "get_available_models",
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
    denial_latched: bool,
    terminal: Option<TerminalOutcome>,
    tools: BTreeMap<String, Option<ToolOutcome>>,
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingUiRequest {
    pub id: String,
    pub method: String,
    pub options: Vec<String>,
}

impl PiTranslator {
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

    #[cfg(test)]
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
        }
    }

    pub fn begin(&mut self, operation: Operation) -> Result<Vec<Outbound>, AdapterError> {
        match operation {
            Operation::Initialize => self.begin_initialize(),
            Operation::GetModels => self.one(PendingKind::Models, Map::new()),
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

    pub(crate) fn reject_before_write(&mut self, command_id: &str) -> Result<(), AdapterError> {
        let kind = *self
            .pending
            .get(command_id)
            .ok_or(AdapterError::Correlation)?;
        if self
            .initialization
            .as_ref()
            .is_some_and(|intent| intent.state_id == command_id || intent.models_id == command_id)
        {
            self.reset_initialization();
            return Ok(());
        }
        self.pending.remove(command_id);
        match kind {
            PendingKind::Prompt => {
                let active = self.active.as_ref().ok_or(AdapterError::Correlation)?;
                if active.prompt_command_id != command_id
                    || active.event_phase != EventPhase::PromptIssued
                {
                    return Err(AdapterError::Correlation);
                }
                self.active = None;
            }
            PendingKind::Abort => {
                self.active
                    .as_mut()
                    .ok_or(AdapterError::Correlation)?
                    .abort_requested = false;
            }
            PendingKind::State
            | PendingKind::Models
            | PendingKind::NewSession
            | PendingKind::Messages
            | PendingKind::Entries => {}
        }
        Ok(())
    }

    fn begin_prompt(
        &mut self,
        turn_key: String,
        message: String,
    ) -> Result<Vec<Outbound>, AdapterError> {
        if self.launch_admission.is_none() {
            return Err(AdapterError::LaunchNotAdmitted);
        }
        validate_id("Turn key", &turn_key)?;
        if message.is_empty() || message.len() > PROMPT_BYTES || message.contains('\0') {
            return Err(AdapterError::Limit("prompt bytes"));
        }
        if self
            .active
            .as_ref()
            .is_some_and(|turn| turn.terminal.is_none())
        {
            return Err(AdapterError::PromptAlreadyActive);
        }
        let mut fields = Map::new();
        fields.insert("message".into(), Value::String(message));
        let outbound = self.issue(PendingKind::Prompt, fields)?;
        self.active = Some(ActiveTurn {
            turn_key,
            prompt_command_id: outbound.command_id.clone(),
            event_phase: EventPhase::PromptIssued,
            abort_requested: false,
            denial_latched: false,
            terminal: None,
            tools: BTreeMap::new(),
        });
        Ok(vec![outbound])
    }

    fn one(
        &mut self,
        kind: PendingKind,
        fields: Map<String, Value>,
    ) -> Result<Vec<Outbound>, AdapterError> {
        Ok(vec![self.issue(kind, fields)?])
    }

    fn issue(
        &mut self,
        kind: PendingKind,
        mut fields: Map<String, Value>,
    ) -> Result<Outbound, AdapterError> {
        if self.pending.len() >= PENDING_COMMANDS {
            return Err(AdapterError::CommandCapacity);
        }
        self.next_command = self
            .next_command
            .checked_add(1)
            .ok_or(AdapterError::CommandCapacity)?;
        let id = format!("pi-{}-{}", self.epoch, self.next_command);
        fields.insert("type".into(), Value::String(kind.command().into()));
        fields.insert("id".into(), Value::String(id.clone()));
        let line =
            serde_json::to_string(&Value::Object(fields)).map_err(|_| AdapterError::Malformed)?;
        if line
            .len()
            .checked_add(1)
            .is_none_or(|wire_bytes| wire_bytes > super::decode::FRAME_BYTES)
        {
            return Err(AdapterError::Limit("outbound frame bytes"));
        }
        self.pending.insert(id.clone(), kind);
        Ok(Outbound {
            command_id: id,
            line,
        })
    }

    fn ingest_response(&mut self, response: Response) -> Result<Vec<AdapterOutput>, AdapterError> {
        let fingerprint = response_fingerprint(&response);
        let Some(kind) = self.pending.remove(&response.id) else {
            return self.classify_repeated_response(&response, fingerprint);
        };
        if response.command != kind.command() {
            if matches!(kind, PendingKind::State | PendingKind::Models) {
                self.reset_initialization();
            }
            return Err(AdapterError::Correlation);
        }
        self.remember_response(&response, fingerprint);
        if !response.success {
            let mut output = vec![AdapterOutput::Error {
                code: "command_failed",
            }];
            if kind == PendingKind::Prompt {
                output.extend(self.finish_turn(TerminalOutcome::Failed)?);
            } else if kind == PendingKind::Abort {
                let active = self.active.as_mut().ok_or(AdapterError::StaleIntent)?;
                active.abort_requested = false;
            } else if matches!(kind, PendingKind::State | PendingKind::Models) {
                self.reset_initialization();
            }
            return Ok(output);
        }
        match kind {
            PendingKind::State => {
                let state = match parse_state(&response.data) {
                    Ok(state) => state,
                    Err(error) => {
                        self.reset_initialization();
                        return Err(error);
                    }
                };
                let mut output = vec![AdapterOutput::State(state)];
                self.note_initialization(&response.id, PendingKind::State, &mut output);
                Ok(output)
            }
            PendingKind::Models => {
                let models = match parse_models(&response.data) {
                    Ok(models) => models,
                    Err(error) => {
                        self.reset_initialization();
                        return Err(error);
                    }
                };
                let mut output = vec![AdapterOutput::Models(models)];
                self.note_initialization(&response.id, PendingKind::Models, &mut output);
                Ok(output)
            }
            PendingKind::NewSession => {
                self.active = None;
                Ok(vec![AdapterOutput::SessionReset])
            }
            PendingKind::Prompt => {
                let active = self.active.as_mut().ok_or(AdapterError::StaleIntent)?;
                if active.prompt_command_id != response.id {
                    return Err(AdapterError::Correlation);
                }
                if active.terminal.is_some() {
                    return Ok(vec![AdapterOutput::LateIgnored]);
                }
                if active.event_phase != EventPhase::PromptIssued {
                    return Err(AdapterError::EventSequence);
                }
                active.event_phase = EventPhase::PromptAccepted;
                Ok(vec![AdapterOutput::CommandAccepted { command: "prompt" }])
            }
            PendingKind::Abort => Ok(vec![AdapterOutput::CommandAccepted { command: "abort" }]),
            PendingKind::Messages => Ok(vec![AdapterOutput::Messages(parse_messages(
                &response.data,
            )?)]),
            PendingKind::Entries => {
                Ok(vec![AdapterOutput::Entries(parse_entries(&response.data)?)])
            }
        }
    }

    fn classify_repeated_response(
        &self,
        response: &Response,
        fingerprint: u64,
    ) -> Result<Vec<AdapterOutput>, AdapterError> {
        match self.completed.get(&response.id) {
            Some(completed)
                if completed.command == response.command
                    && completed.success == response.success
                    && completed.payload_fingerprint == fingerprint =>
            {
                Ok(vec![AdapterOutput::DuplicateIgnored])
            }
            Some(_) => Err(AdapterError::Correlation),
            None => Err(AdapterError::Correlation),
        }
    }

    fn remember_response(&mut self, response: &Response, fingerprint: u64) {
        if self.completed.len() == COMPLETED_COMMANDS
            && let Some(oldest) = self.completed_order.pop_front()
        {
            self.completed.remove(&oldest);
        }
        self.completed.insert(
            response.id.clone(),
            CompletedResponse {
                command: response.command.clone(),
                success: response.success,
                payload_fingerprint: fingerprint,
            },
        );
        self.completed_order.push_back(response.id.clone());
    }
}

fn validate_id(field: &'static str, value: &str) -> Result<(), AdapterError> {
    validate_opaque_text(value, EXTERNAL_ID_BYTES).map_err(|_| AdapterError::Limit(field))
}
