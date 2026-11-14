use serde_json::json;

use crate::acp::contract::RequiredText;

use super::{OutgoingRequest, RequestError, RequestKind};

impl OutgoingRequest {
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
