use serde_json::json;

use crate::acp::{
    contract::RequiredText,
    session_action::{ReviewTarget, SessionAction},
};

use super::{OutgoingRequest, RequestError, RequestKind};

impl OutgoingRequest {
    /// `thread/compact/start` or an inline `review/start` on the bound thread.
    pub(crate) fn session_action(
        id: u64,
        thread_id: &RequiredText<512>,
        action: &SessionAction,
    ) -> Result<Self, RequestError> {
        match action {
            SessionAction::Compact {} => Self::build(
                id,
                RequestKind::Compact,
                json!({"threadId": thread_id.as_str()}),
            ),
            SessionAction::Review {
                target: ReviewTarget::UncommittedChanges {},
            } => Self::build(
                id,
                RequestKind::Review,
                json!({
                    "threadId": thread_id.as_str(),
                    "target": {"type": "uncommittedChanges"},
                    "delivery": "inline"
                }),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(action: serde_json::Value) -> (String, serde_json::Value) {
        let thread = RequiredText::new("threadId", "thread-1").unwrap();
        let action: SessionAction = serde_json::from_value(action).unwrap();
        let request = OutgoingRequest::session_action(7, &thread, &action).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&request.bytes).unwrap();
        assert!(request.kind.starts_turn());
        (
            value["method"].as_str().unwrap().to_owned(),
            value["params"].clone(),
        )
    }

    #[test]
    fn compact_and_review_requests_match_the_pinned_schema() {
        assert_eq!(
            params(json!({"kind": "compact"})),
            (
                "thread/compact/start".to_owned(),
                json!({"threadId": "thread-1"})
            )
        );
        assert_eq!(
            params(json!({"kind": "review", "target": {"kind": "uncommittedChanges"}})),
            (
                "review/start".to_owned(),
                json!({
                    "threadId": "thread-1",
                    "target": {"type": "uncommittedChanges"},
                    "delivery": "inline"
                })
            )
        );
    }
}
