use super::super::wire::WireError;
use crate::acp::contract::ContractError;

#[derive(Debug, thiserror::Error)]
pub(crate) enum TranslationError {
    #[error("Claude ACP frame is malformed")]
    MalformedFrame,
    #[error("Claude ACP method is not valid for this translator entry point")]
    UnexpectedMethod,
    #[error("Claude ACP event belongs to a foreign session, request, or tool")]
    ForeignId,
    #[error("Claude ACP prompt already has an exact pending request")]
    PromptAlreadyActive,
    #[error("Claude ACP tool update has no correlated tool call")]
    OrphanToolUpdate,
    #[error("Claude ACP repeated a tool call with conflicting state")]
    DuplicateTool,
    #[error("Claude ACP tool transition is invalid")]
    InvalidToolTransition,
    #[error("Claude ACP terminal conflicts with the committed terminal")]
    TerminalConflict,
    #[error("Claude ACP tool state is unknown")]
    UnknownToolState,
    #[error("Claude ACP prompt terminal is unknown")]
    UnknownTerminal,
    #[error("Claude ACP tool count exceeded the per-Turn limit")]
    ToolLimit,
    #[error("Claude ACP Item snapshot exceeded its per-Turn count or byte limit")]
    ItemLimit,
    #[error("Claude ACP permission count exceeded the per-session limit")]
    PermissionLimit,
    #[error("Claude ACP plan exceeded its count or byte limit")]
    PlanLimit,
    #[error("Claude ACP frame expanded to too many normalized events")]
    EventLimit,
    #[error("Claude ACP path was foreign, ambiguous, missing, or noncanonical")]
    Resource,
    #[error(transparent)]
    Contract(#[from] ContractError),
    #[error(transparent)]
    Wire(#[from] WireError),
}
