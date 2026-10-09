pub(super) fn is_allow_option(option: &str) -> bool {
    option.eq_ignore_ascii_case("allow") || option.eq_ignore_ascii_case("allow once")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operation {
    Initialize,
    GetModels,
    SetModel { provider: String, id: String },
    GetThinkingLevels,
    SetThinkingLevel { level: String },
    NewSession,
    Prompt { turn_key: String, message: String },
    Abort,
    GetState,
    GetMessages,
    GetEntries { since: Option<String> },
    Resume,
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateSnapshot {
    pub model: Option<ModelSummary>,
    pub thinking_level: Option<String>,
    pub has_session: bool,
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
    SessionReset,
    State(StateSnapshot),
    Models(Vec<ModelSummary>),
    ModelSelected(ModelSummary),
    ThinkingLevels(Vec<String>),
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
