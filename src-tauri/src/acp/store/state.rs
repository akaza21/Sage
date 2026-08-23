use crate::acp::lifecycle::{
    ActivityLifecycle, ItemLifecycle, ServerRequestState, SessionState, ToolState,
};

use super::StoreError;

macro_rules! parse_state {
    ($name:ident,$ty:ty,{$($text:literal=>$variant:expr),+$(,)?}) => {
        pub(super) fn $name(value:&str)->Result<$ty,StoreError>{match value{
            $($text=>Ok($variant),)+ _=>Err(StoreError::Corrupt("unknown lifecycle state"))
        }}
    };
}

parse_state!(parse_session_state,SessionState,{"created"=>SessionState::Created,"starting"=>SessionState::Starting,"ready"=>SessionState::Ready,"active"=>SessionState::Active,"recovering"=>SessionState::Recovering,"closing"=>SessionState::Closing,"closed"=>SessionState::Closed,"failed"=>SessionState::Failed,"incompatible"=>SessionState::Incompatible});
parse_state!(parse_item_lifecycle,ItemLifecycle,{"started"=>ItemLifecycle::Started,"streaming"=>ItemLifecycle::Streaming,"completed"=>ItemLifecycle::Completed,"failed"=>ItemLifecycle::Failed,"cancelled"=>ItemLifecycle::Cancelled,"unsupported"=>ItemLifecycle::Unsupported});
parse_state!(parse_tool_state,ToolState,{"pending"=>ToolState::Pending,"inProgress"=>ToolState::InProgress,"waitingForApproval"=>ToolState::WaitingForApproval,"completed"=>ToolState::Completed,"failed"=>ToolState::Failed,"declined"=>ToolState::Declined,"cancelled"=>ToolState::Cancelled});
parse_state!(parse_activity_lifecycle,ActivityLifecycle,{"observed"=>ActivityLifecycle::Observed,"active"=>ActivityLifecycle::Active,"completed"=>ActivityLifecycle::Completed,"failed"=>ActivityLifecycle::Failed,"cancelled"=>ActivityLifecycle::Cancelled,"unsupported"=>ActivityLifecycle::Unsupported});
parse_state!(parse_request_state,ServerRequestState,{"received"=>ServerRequestState::Received,"asked"=>ServerRequestState::Asked,"allowPending"=>ServerRequestState::AllowPending,"allowCommitted"=>ServerRequestState::AllowCommitted,"declinePending"=>ServerRequestState::DeclinePending,"declineCommitted"=>ServerRequestState::DeclineCommitted,"responseQueued"=>ServerRequestState::ResponseQueued,"resolvedAllowed"=>ServerRequestState::ResolvedAllowed,"resolvedDeclined"=>ServerRequestState::ResolvedDeclined});

pub(super) fn session_state_text(value: SessionState) -> &'static str {
    match value {
        SessionState::Created => "created",
        SessionState::Starting => "starting",
        SessionState::Ready => "ready",
        SessionState::Active => "active",
        SessionState::Recovering => "recovering",
        SessionState::Closing => "closing",
        SessionState::Closed => "closed",
        SessionState::Failed => "failed",
        SessionState::Incompatible => "incompatible",
    }
}
pub(super) fn item_lifecycle_text(value: ItemLifecycle) -> &'static str {
    match value {
        ItemLifecycle::Started => "started",
        ItemLifecycle::Streaming => "streaming",
        ItemLifecycle::Completed => "completed",
        ItemLifecycle::Failed => "failed",
        ItemLifecycle::Cancelled => "cancelled",
        ItemLifecycle::Unsupported => "unsupported",
    }
}
pub(super) fn tool_state_text(value: ToolState) -> &'static str {
    match value {
        ToolState::Pending => "pending",
        ToolState::InProgress => "inProgress",
        ToolState::WaitingForApproval => "waitingForApproval",
        ToolState::Completed => "completed",
        ToolState::Failed => "failed",
        ToolState::Declined => "declined",
        ToolState::Cancelled => "cancelled",
    }
}
pub(super) fn activity_lifecycle_text(value: ActivityLifecycle) -> &'static str {
    match value {
        ActivityLifecycle::Observed => "observed",
        ActivityLifecycle::Active => "active",
        ActivityLifecycle::Completed => "completed",
        ActivityLifecycle::Failed => "failed",
        ActivityLifecycle::Cancelled => "cancelled",
        ActivityLifecycle::Unsupported => "unsupported",
    }
}
pub(super) fn request_state_text(value: ServerRequestState) -> &'static str {
    match value {
        ServerRequestState::Received => "received",
        ServerRequestState::Asked => "asked",
        ServerRequestState::AllowPending => "allowPending",
        ServerRequestState::AllowCommitted => "allowCommitted",
        ServerRequestState::DeclinePending => "declinePending",
        ServerRequestState::DeclineCommitted => "declineCommitted",
        ServerRequestState::ResponseQueued => "responseQueued",
        ServerRequestState::ResolvedAllowed => "resolvedAllowed",
        ServerRequestState::ResolvedDeclined => "resolvedDeclined",
    }
}
