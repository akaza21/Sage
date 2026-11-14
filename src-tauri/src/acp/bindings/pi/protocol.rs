pub(super) fn is_allow_option(option: &str) -> bool {
    option.eq_ignore_ascii_case("allow") || option.eq_ignore_ascii_case("allow once")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operation {
    Initialize,
    GetModels,
    SetModel {
        provider: String,
        id: String,
    },
    GetThinkingLevels,
    GetCommands,
    SetThinkingLevel {
        level: String,
    },
    /// Pi's own setting for compacting when the context is nearly full.
    SetAutoCompaction {
        enabled: bool,
    },
    NewSession,
    Prompt {
        turn_key: String,
        message: String,
        /// Pi `ImageContent` blocks: `type`, base64 `data` and `mimeType`.
        images: Vec<serde_json::Value>,
    },
    Compact {
        turn_key: String,
        instructions: Option<String>,
    },
    Steer {
        message: String,
    },
    ClearQueue,
    Abort,
    GetState,
    GetMessages,
    GetEntries {
        since: Option<String>,
    },
    Resume {
        session_id: String,
        path: String,
    },
    Load,
    Close,
    MultiRoot,
    ForkOrClone,
    /// A request outside any Turn whose answer goes back to the caller that
    /// asked for it, never into the transcript.
    Native(NativeRequest),
}

/// Requests Sage makes of Pi's session on behalf of a native RPC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeRequest {
    /// Token and cost totals for the whole session (`get_session_stats`).
    SessionStats,
    /// The display name Pi lists the session under (`set_session_name`).
    SessionName { name: String },
    /// Stops waiting to retry a failed reply (`abort_retry`).
    AbortRetry,
    /// Copies the session at its current position into a new session and
    /// moves Pi onto the copy (`clone`).
    Clone,
    /// Reads which session Pi is on after a clone (`get_state`).
    ForkState,
    /// Moves Pi back onto the session it was on (`switch_session`).
    ReturnToSession { path: String },
}

impl NativeRequest {
    pub(crate) const fn command(&self) -> NativeCommand {
        match self {
            Self::SessionStats => NativeCommand::SessionStats,
            Self::SessionName { .. } => NativeCommand::SessionName,
            Self::AbortRetry => NativeCommand::AbortRetry,
            Self::Clone => NativeCommand::Clone,
            Self::ForkState => NativeCommand::ForkState,
            Self::ReturnToSession { .. } => NativeCommand::ReturnToSession,
        }
    }

    pub(crate) fn into_fields(self) -> serde_json::Map<String, serde_json::Value> {
        let mut fields = serde_json::Map::new();
        match self {
            Self::SessionStats | Self::AbortRetry | Self::Clone | Self::ForkState => {}
            Self::ReturnToSession { path } => {
                fields.insert("sessionPath".into(), serde_json::Value::String(path));
            }
            Self::SessionName { name } => {
                fields.insert("name".into(), serde_json::Value::String(name));
            }
        }
        fields
    }
}

/// Which native request a reply answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeCommand {
    SessionStats,
    SessionName,
    AbortRetry,
    Clone,
    ForkState,
    ReturnToSession,
}

impl NativeCommand {
    pub(crate) const fn wire(self) -> &'static str {
        match self {
            Self::SessionStats => "get_session_stats",
            Self::SessionName => "set_session_name",
            Self::AbortRetry => "abort_retry",
            Self::Clone => "clone",
            Self::ForkState => "get_state",
            Self::ReturnToSession => "switch_session",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outbound {
    pub command_id: String,
    pub line: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelSummary {
    pub provider: String,
    pub id: String,
    pub name: String,
    pub context_window: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateSnapshot {
    pub model: Option<ModelSummary>,
    pub thinking_level: Option<String>,
    /// Pi's id for its open session, and the file it keeps it in.
    pub session_id: Option<String>,
    pub session_file: Option<String>,
    pub is_streaming: bool,
    /// Whether Pi compacts on its own when the context is nearly full.
    pub auto_compaction: Option<bool>,
}

impl StateSnapshot {
    /// The chat's key for Pi's session: its file's name, `<timestamp>_<id>`,
    /// so reopening the chat opens that file directly. Pi's bare id stands in
    /// when it keeps no file.
    pub fn session_key(&self) -> Option<String> {
        self.session_file
            .as_deref()
            .and_then(session_file_key)
            .map(str::to_owned)
            .or_else(|| self.session_id.clone())
    }
}

/// The chat key Pi's session file names: `<timestamp>_<id>.jsonl`.
pub fn session_file_key(file: &str) -> Option<&str> {
    std::path::Path::new(file)
        .file_name()?
        .to_str()?
        .strip_suffix(".jsonl")
        .filter(|stem| is_session_key(stem))
}

/// Pi's session ids and file names use only these characters, so a key can
/// never name a path outside Pi's session folder.
pub fn is_session_key(key: &str) -> bool {
    (1..=128).contains(&key.len())
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryMessage {
    pub role: HistoryRole,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryRole {
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntrySummary {
    pub external_id: String,
    pub kind: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalOutcome {
    Completed,
    /// Sage's user asked Pi to stop.
    Cancelled,
    /// Pi, or Sage's authority policy, stopped the Turn on its own.
    Interrupted,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolOutcome {
    Completed,
    Failed,
    Declined,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdapterOutput {
    InitializationProgress,
    Initialized,
    CommandAccepted {
        command: &'static str,
    },
    /// Pi's session, by its own id, is open for this chat.
    SessionOpened(String),
    State(StateSnapshot),
    Models(Vec<ModelSummary>),
    ModelSelected(ModelSummary),
    ThinkingLevels(Vec<String>),
    Commands(serde_json::Value),
    ThinkingSelected(String),
    AutoCompactionSelected(bool),
    Messages(Vec<HistoryMessage>),
    Entries(Vec<EntrySummary>),
    TurnRunning {
        turn_key: String,
    },
    TurnTerminal {
        turn_key: String,
        outcome: TerminalOutcome,
    },
    FailureDetail {
        message: Option<String>,
    },
    AttemptFailed {
        attempt: u64,
        max_attempts: Option<u64>,
        message: String,
    },
    /// Pi waits to retry a failed reply, or has stopped waiting.
    Retry {
        attempt: u64,
        max_attempts: Option<u64>,
        state: crate::acp::intent::session_event::SessionActivityState,
    },
    TextStarted {
        external_id: Option<String>,
    },
    TextDelta {
        text: String,
        redacted_tail: bool,
    },
    ThoughtDelta {
        text: String,
        redacted_tail: bool,
    },
    ThoughtStarted,
    ThoughtCompleted {
        outcome: TerminalOutcome,
    },
    TextCompleted {
        external_id: Option<String>,
        outcome: TerminalOutcome,
    },
    CompactionStarted,
    CompactionCompleted,
    CompactionInterrupted,
    CompactionFailed,
    ToolStarted {
        external_id: String,
        name: String,
        path: Option<String>,
    },
    ToolUpdated {
        external_id: String,
        output: Option<String>,
    },
    ToolTerminal {
        external_id: String,
        outcome: ToolOutcome,
        output: Option<String>,
    },
    Image {
        media_type: String,
        data: String,
    },
    Usage {
        input_tokens: u64,
        output_tokens: u64,
    },
    PermissionRequested {
        external_id: String,
    },
    SteerResolved {
        command_id: String,
        accepted: bool,
    },
    Notice {
        level: crate::acp::intent::session_event::SessionNoticeLevel,
        source: &'static str,
        text: String,
    },
    Title(String),
    DialogCancelled {
        external_id: String,
    },
    /// A dialog Pi timed out on its own, replaced by a newer one.
    DialogExpired {
        external_id: String,
    },
    AuthorityBlocked {
        external_id: String,
        tool: String,
    },
    Error {
        code: &'static str,
    },
    Unsupported {
        kind: String,
    },
    UnknownEventsSuppressed,
    /// Pi answered a native request; a refusal carries no data.
    NativeReply {
        command_id: String,
        command: NativeCommand,
        data: Option<serde_json::Value>,
    },
    DuplicateIgnored,
    LateIgnored,
    Absorbed,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AdapterError {
    #[error("Pi rejected the session configuration mutation")]
    ConfigMutationRejected,
    #[error("Pi operation is not supported by the pinned RPC binding: {0}")]
    UnsupportedOperation(&'static str),
    #[error("Pi prompting is unavailable until the curated artifact graph is admitted")]
    LaunchNotAdmitted,
    #[error("Pi command capacity is exhausted")]
    CommandCapacity,
    #[error("a Pi prompt is already active")]
    PromptAlreadyActive,
    #[error("Pi initialization is already active")]
    InitializationAlreadyActive,
    #[error("Pi session transition is blocked by active or pending work")]
    SessionTransitionBusy,
    #[error("Pi protocol record is malformed")]
    Malformed,
    #[error("Pi protocol record exceeds a mechanical limit: {0}")]
    Limit(&'static str),
    #[error("Pi response has a missing, foreign, duplicate, or mismatched command ID")]
    Correlation,
    #[error("Pi emitted activity without the exact active Turn")]
    StaleIntent,
    #[error("Pi ID-less event violated the active Turn grammar")]
    EventSequence,
    #[error("Pi emitted a conflicting terminal update")]
    TerminalConflict,
    #[error("Pi attempted a tool outside the verified read-only allowlist")]
    PermissionViolation,
}
