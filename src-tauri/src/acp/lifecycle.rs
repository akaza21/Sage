use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SessionState {
    Created,
    Starting,
    Ready,
    Active,
    Recovering,
    Closing,
    Closed,
    Failed,
    Incompatible,
}

impl SessionState {
    pub fn can_transition_to(self, next: Self) -> bool {
        use SessionState::*;
        matches!(
            (self, next),
            (Created, Starting | Closing)
                | (Starting, Created | Ready | Incompatible | Failed | Closing)
                | (Ready, Active | Recovering | Closing | Failed)
                | (Active, Ready | Recovering | Closing | Failed)
                | (Recovering, Ready | Incompatible | Failed | Closing)
                | (Closing, Closed | Failed)
        )
    }
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Closed | Self::Failed | Self::Incompatible)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Residency {
    Hot,
    Cold,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeEpochState {
    Legacy,
    Starting,
    Running,
    Terminating,
    Exited,
    Failed,
    Aborted,
}

impl RuntimeEpochState {
    pub fn can_transition_to(self, next: Self) -> bool {
        use RuntimeEpochState::*;
        matches!(
            (self, next),
            (Starting, Running | Terminating | Failed | Aborted)
                | (Running, Terminating | Exited | Failed)
                | (Terminating, Exited | Failed)
        )
    }
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Legacy | Self::Exited | Self::Failed | Self::Aborted
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TurnState {
    Queued,
    Sending,
    Running,
    WaitingModel,
    WaitingTool,
    WaitingApproval,
    WaitingUser,
    Cancelling,
    Completed,
    Interrupted,
    Cancelled,
    Failed,
    DeliveryUnknown,
}

impl TurnState {
    pub fn can_transition_to(self, next: Self) -> bool {
        use TurnState::*;
        match self {
            Queued => matches!(next, Sending | Cancelled),
            Sending => matches!(
                next,
                Running | Cancelling | Cancelled | Failed | DeliveryUnknown
            ),
            Running => matches!(
                next,
                WaitingModel
                    | WaitingTool
                    | WaitingApproval
                    | WaitingUser
                    | Cancelling
                    | Completed
                    | Interrupted
                    | Cancelled
                    | Failed
            ),
            WaitingModel | WaitingTool | WaitingApproval | WaitingUser => matches!(
                next,
                Running | Cancelling | Completed | Interrupted | Cancelled | Failed
            ),
            Cancelling => matches!(next, Cancelled | Interrupted | Failed | DeliveryUnknown),
            Completed | Interrupted | Cancelled | Failed | DeliveryUnknown => false,
        }
    }
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed
                | Self::Interrupted
                | Self::Cancelled
                | Self::Failed
                | Self::DeliveryUnknown
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DeliveryState {
    NotWritten,
    Writing,
    ZeroBytes,
    PossibleBytes,
    CompleteBytes,
    Correlated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ItemLifecycle {
    Started,
    Streaming,
    Completed,
    Failed,
    Cancelled,
    Unsupported,
}

impl ItemLifecycle {
    pub fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (
                Self::Started,
                Self::Streaming
                    | Self::Completed
                    | Self::Failed
                    | Self::Cancelled
                    | Self::Unsupported
            ) | (
                Self::Streaming,
                Self::Completed | Self::Failed | Self::Cancelled | Self::Unsupported
            )
        )
    }
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled | Self::Unsupported
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolState {
    Pending,
    InProgress,
    WaitingForApproval,
    Completed,
    Failed,
    Declined,
    Cancelled,
}

impl ToolState {
    pub fn can_transition_to(self, next: Self) -> bool {
        use ToolState::*;
        matches!(
            (self, next),
            (
                Pending,
                InProgress | WaitingForApproval | Completed | Failed | Declined | Cancelled
            ) | (
                InProgress,
                WaitingForApproval | Completed | Failed | Declined | Cancelled
            ) | (
                WaitingForApproval,
                InProgress | Completed | Failed | Declined | Cancelled
            )
        )
    }
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Declined | Self::Cancelled
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ItemKind {
    Text,
    ThoughtSummary,
    Plan,
    ToolCall,
    ToolUpdateResult,
    FileChangeDiff,
    Usage,
    PermissionRequest,
    Error,
    Unsupported,
    Collaboration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ServerRequestState {
    Received,
    DeclinePending,
    DeclineCommitted,
    ResponseQueued,
    ResolvedDeclined,
}

impl ServerRequestState {
    pub fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Received, Self::DeclinePending)
                | (Self::DeclinePending, Self::DeclineCommitted)
                | (Self::DeclineCommitted, Self::ResponseQueued)
                | (Self::ResponseQueued, Self::ResolvedDeclined)
        )
    }
    pub fn is_terminal(self) -> bool {
        self == Self::ResolvedDeclined
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ActivityLifecycle {
    Observed,
    Active,
    Completed,
    Failed,
    Cancelled,
    Unsupported,
}

impl ActivityLifecycle {
    pub fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (
                Self::Observed,
                Self::Active | Self::Completed | Self::Failed | Self::Cancelled | Self::Unsupported
            ) | (
                Self::Active,
                Self::Completed | Self::Failed | Self::Cancelled | Self::Unsupported
            )
        )
    }
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled | Self::Unsupported
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalUpdate {
    Apply,
    NoOp,
    Conflict,
}

pub fn classify_terminal<T: Eq>(current: Option<&T>, incoming: &T) -> TerminalUpdate {
    match current {
        None => TerminalUpdate::Apply,
        Some(current) if current == incoming => TerminalUpdate::NoOp,
        Some(_) => TerminalUpdate::Conflict,
    }
}
