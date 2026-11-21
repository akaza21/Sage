use super::{OutgoingRequest, RequestError, RequestKind};
use sage_acp_protocol::contract::RequiredText;
use serde_json::json;

impl OutgoingRequest {
    /// The pinned schema requires only `threadId`; omitted overrides inherit
    /// the source thread, and `lastTurnId` is left out because Sage forks only
    /// between Turns. Sage imports no native history, so the forked Turns are
    /// excluded; asking for them draws a `deprecationNotice` from the pinned server.
    pub(crate) fn fork(id: u64, thread_id: &RequiredText<512>) -> Result<Self, RequestError> {
        Self::fork_at(id, thread_id, None)
    }

    pub(crate) fn fork_at(
        id: u64,
        thread_id: &RequiredText<512>,
        last_turn_id: Option<&str>,
    ) -> Result<Self, RequestError> {
        let mut params = json!({"threadId":thread_id.as_str(),"excludeTurns":true});
        if let Some(turn) = last_turn_id {
            params["lastTurnId"] = json!(turn);
        }
        Self::build(id, RequestKind::Fork, params)
    }
}
