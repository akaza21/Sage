use serde_json::json;

use sage_acp_protocol::contract::RequiredText;

use super::{OutgoingRequest, RequestError, RequestKind};
use crate::acp::goal::{GoalAction, valid_objective};

impl OutgoingRequest {
    pub(crate) fn goal_set(
        id: u64,
        thread_id: &RequiredText<512>,
        action: &GoalAction,
    ) -> Result<Self, RequestError> {
        let mut params = json!({"threadId": thread_id.as_str()});
        match action {
            GoalAction::Set { objective, .. }
                if valid_objective(objective) && !objective.trim().is_empty() =>
            {
                params["objective"] = json!(objective);
            }
            GoalAction::ResumeExisting => params["status"] = json!("active"),
            _ => return Err(RequestError::InvalidGoal),
        }
        Self::build(id, RequestKind::GoalSet, params)
    }
    pub(crate) fn goal_get(id: u64, thread_id: &RequiredText<512>) -> Result<Self, RequestError> {
        Self::build(
            id,
            RequestKind::GoalGet,
            json!({"threadId": thread_id.as_str()}),
        )
    }

    pub(crate) fn goal_pause(id: u64, thread_id: &RequiredText<512>) -> Result<Self, RequestError> {
        Self::build(
            id,
            RequestKind::GoalPause,
            json!({"threadId": thread_id.as_str(), "status": "paused"}),
        )
    }

    pub(crate) fn goal_clear(id: u64, thread_id: &RequiredText<512>) -> Result<Self, RequestError> {
        Self::build(
            id,
            RequestKind::GoalClear,
            json!({"threadId": thread_id.as_str()}),
        )
    }
}
