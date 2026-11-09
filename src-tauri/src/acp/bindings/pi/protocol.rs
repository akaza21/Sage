pub(super) fn is_allow_option(option: &str) -> bool {
    option.eq_ignore_ascii_case("allow") || option.eq_ignore_ascii_case("allow once")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operation {
    Initialize,
    GetModels,
    SetModel { provider: String, id: String },
    GetThinkingLevels,
    GetCommands,
    SetThinkingLevel { level: String },
    NewSession,
    Prompt { turn_key: String, message: String },
    Steer { message: String },
    ClearQueue,
    Abort,
    GetState,
    GetMessages,
    GetEntries { since: Option<String> },
    Resume { session_id: String, path: String },
    Load,
    Close,
    MultiRoot,
    ForkOrClone,
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
