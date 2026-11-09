use serde_json::json;

use crate::acp::contract::RequiredText;

use super::{NativeProjectRoot, OutgoingRequest, RequestError, RequestKind};

impl OutgoingRequest {
    /// One page of a thread's Turns with every item, oldest first or newest
    /// first.
    pub(crate) fn turns_list(
        id: u64,
        thread_id: &RequiredText<512>,
        cursor: Option<&str>,
        newest_first: bool,
    ) -> Result<Self, RequestError> {
        Self::build(
            id,
            RequestKind::TurnsList,
            json!({
                "threadId": thread_id.as_str(),
                "cursor": cursor,
                "limit": super::super::child_transcript::TURN_PAGE,
                "itemsView": "full",
                "sortDirection": if newest_first { "desc" } else { "asc" }
            }),
        )
    }

    /// One page of the unarchived CLI sessions started in `root`, most
    /// recently active first.
    pub(crate) fn session_list(
        id: u64,
        root: &NativeProjectRoot,
        cursor: Option<&str>,
    ) -> Result<Self, RequestError> {
        Self::build(
            id,
            RequestKind::SessionList,
            json!({
                "cwd": root.as_str(),
                "sourceKinds": ["cli"],
                "archived": false,
                "cursor": cursor,
                "limit": super::super::child_transcript::SESSION_PAGE,
                "sortKey": "updated_at",
                "sortDirection": "desc"
            }),
        )
    }
}
