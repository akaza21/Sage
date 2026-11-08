use serde_json::{Value, json};

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
            SessionAction::Review { target } => Self::build(
                id,
                RequestKind::Review,
                json!({
                    "threadId": thread_id.as_str(),
                    "target": review_target(target),
                    "delivery": "inline"
                }),
            ),
        }
    }
}

/// The 0.167 `ReviewTarget`: the same variants, tagged by `type`.
fn review_target(target: &ReviewTarget) -> Value {
    match target {
        ReviewTarget::UncommittedChanges {} => json!({"type": "uncommittedChanges"}),
        ReviewTarget::BaseBranch { branch } => {
            json!({"type": "baseBranch", "branch": branch.as_str()})
        }
        ReviewTarget::Commit { sha, title } => {
            let mut target = json!({"type": "commit", "sha": sha.as_str()});
            if let Some(title) = title {
                target["title"] = json!(title.as_str());
            }
            target
        }
        ReviewTarget::Custom { instructions } => {
            json!({"type": "custom", "instructions": instructions.as_str()})
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
        for (target, wire) in [
            (
                json!({"kind": "uncommittedChanges"}),
                json!({"type": "uncommittedChanges"}),
            ),
            (
                json!({"kind": "baseBranch", "branch": "main"}),
                json!({"type": "baseBranch", "branch": "main"}),
            ),
            (
                json!({"kind": "commit", "sha": "abc123"}),
                json!({"type": "commit", "sha": "abc123"}),
            ),
            (
                json!({"kind": "commit", "sha": "abc123", "title": "Fix the parser"}),
                json!({"type": "commit", "sha": "abc123", "title": "Fix the parser"}),
            ),
            (
                json!({"kind": "custom", "instructions": "Check the error paths"}),
                json!({"type": "custom", "instructions": "Check the error paths"}),
            ),
        ] {
            assert_eq!(
                params(json!({"kind": "review", "target": target})),
                (
                    "review/start".to_owned(),
                    json!({"threadId": "thread-1", "target": wire, "delivery": "inline"})
                )
            );
        }
    }
}
