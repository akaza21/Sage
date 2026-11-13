use serde_json::json;

use super::{OutgoingRequest, RequestError, RequestKind};
use crate::acp::contract::RequiredText;

impl OutgoingRequest {
    pub(crate) fn thread_revert(
        id: u64,
        thread_id: &RequiredText<512>,
        before_turn_id: &str,
    ) -> Result<Self, RequestError> {
        Self::build(
            id,
            RequestKind::ThreadRevert,
            json!({"threadId": thread_id.as_str(), "beforeTurnId": before_turn_id}),
        )
    }
}
