//! Harness-neutral session actions. Each runs as an ordinary Turn on an idle
//! session whose negotiated capabilities admit it; a binding maps it to its
//! own native method.
use serde::{Deserialize, Serialize};

use super::{contract::RequiredText, intent::CapabilitySnapshotV1};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum SessionAction {
    Compact {},
    Review { target: ReviewTarget },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
/// What a review covers, as Codex's `ReviewTarget` names it.
pub(crate) enum ReviewTarget {
    UncommittedChanges {},
    BaseBranch {
        branch: RequiredText<256>,
    },
    Commit {
        sha: RequiredText<64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<RequiredText<512>>,
    },
    Custom {
        instructions: RequiredText<16384>,
    },
}

/// The `/` commands that name session actions, as their prompts read.
pub(crate) const COMMAND_NAMES: [&str; 2] = ["compact", "review"];

impl SessionAction {
    pub(crate) fn admitted_by(&self, capabilities: &CapabilitySnapshotV1) -> bool {
        match self {
            Self::Compact {} => capabilities.native_session_compact,
            Self::Review { .. } => capabilities.native_review,
        }
    }

    /// The Turn's user-visible prompt, in the form a `/` command takes.
    pub(crate) fn prompt_text(&self) -> String {
        match self {
            Self::Compact {} => "/compact".to_owned(),
            Self::Review { target } => match target {
                ReviewTarget::UncommittedChanges {} => "/review".to_owned(),
                ReviewTarget::BaseBranch { branch } => format!("/review {}", branch.as_str()),
                ReviewTarget::Commit { sha, .. } => format!("/review {}", sha.as_str()),
                ReviewTarget::Custom { instructions } => {
                    format!("/review {}", instructions.as_str())
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn actions_decode_only_bounded_known_targets() {
        let review: SessionAction =
            serde_json::from_value(json!({"kind":"review","target":{"kind":"uncommittedChanges"}}))
                .unwrap();
        assert_eq!(review.prompt_text(), "/review");
        assert!(COMMAND_NAMES.contains(&&review.prompt_text()[1..]));
        for (target, prompt) in [
            (json!({"kind":"baseBranch","branch":"main"}), "/review main"),
            (json!({"kind":"commit","sha":"abc123"}), "/review abc123"),
            (
                json!({"kind":"commit","sha":"abc123","title":"Fix the parser"}),
                "/review abc123",
            ),
            (
                json!({"kind":"custom","instructions":"Check the error paths"}),
                "/review Check the error paths",
            ),
        ] {
            let value = json!({"kind":"review","target":target});
            let action: SessionAction = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(action.prompt_text(), prompt);
            assert_eq!(serde_json::to_value(&action).unwrap(), value);
        }
        assert_eq!(
            serde_json::from_value::<SessionAction>(json!({"kind":"compact"}))
                .unwrap()
                .prompt_text(),
            "/compact"
        );
        for invalid in [
            json!({"kind":"review","target":{"kind":"baseBranch","branch":""}}),
            json!({"kind":"review","target":{"kind":"baseBranch"}}),
            json!({"kind":"review","target":{"kind":"commit","sha":"a".repeat(65)}}),
            json!({"kind":"review","target":{"kind":"commit","sha":"abc123","title":""}}),
            json!({"kind":"review","target":{"kind":"custom","instructions":"x".repeat(16385)}}),
            json!({"kind":"review","target":{"kind":"custom","instructions":"Check","extra":1}}),
            json!({"kind":"review","target":{"kind":"pullRequest","number":1}}),
            json!({"kind":"review"}),
            json!({"kind":"review","target":{"kind":"uncommittedChanges","extra":1}}),
            json!({"kind":"compact","extra":1}),
            json!({"kind":"revert"}),
        ] {
            assert!(serde_json::from_value::<SessionAction>(invalid).is_err());
        }
    }

    #[test]
    fn an_action_turn_keeps_its_action_beside_the_prompt_text() {
        let action: SessionAction =
            serde_json::from_value(json!({"kind":"review","target":{"kind":"uncommittedChanges"}}))
                .unwrap();
        let mut prompt = crate::acp::intent::Prompt::new("prompt", action.prompt_text()).unwrap();
        prompt.action = Some(action.clone());
        let connection = rusqlite::Connection::open_in_memory().unwrap();
        connection
            .execute_batch("CREATE TABLE items(turn_id TEXT,ordinal INTEGER,kind TEXT,payload_json TEXT,payload_bytes INTEGER)")
            .unwrap();
        let payload = json!({"kind":"text","payload":prompt.user_payload()}).to_string();
        connection
            .execute(
                "INSERT INTO items VALUES ('turn',0,'text',?1,?2)",
                rusqlite::params![payload, payload.len() as i64],
            )
            .unwrap();
        let metadata =
            crate::acp::transcript_quote::load_prompt_metadata(&connection, "turn").unwrap();
        assert_eq!(
            metadata.session_action,
            Some(serde_json::to_value(&action).unwrap())
        );
        assert!(crate::acp::store::parse_legacy_text(&payload).is_ok());
        let plain = crate::acp::intent::Prompt::new("prompt", "/review").unwrap();
        assert!(plain.user_payload().get("sessionAction").is_none());
    }

    #[test]
    fn a_stored_action_this_build_does_not_run_still_loads_as_stored() {
        let connection = rusqlite::Connection::open_in_memory().unwrap();
        connection
            .execute_batch("CREATE TABLE items(turn_id TEXT,ordinal INTEGER,kind TEXT,payload_json TEXT,payload_bytes INTEGER)")
            .unwrap();
        let payload = json!({"kind":"text","payload":{"role":"user","text":"/review 42",
            "sessionAction":{"kind":"review","target":{"kind":"pullRequest","number":42}}}})
        .to_string();
        connection
            .execute(
                "INSERT INTO items VALUES ('turn',0,'text',?1,?2)",
                rusqlite::params![payload, payload.len() as i64],
            )
            .unwrap();
        let action = crate::acp::transcript_quote::load_prompt_metadata(&connection, "turn")
            .unwrap()
            .session_action;
        assert_eq!(
            action,
            Some(json!({"kind":"review","target":{"kind":"pullRequest","number":42}}))
        );
        assert!(crate::acp::store::parse_legacy_text(&payload).is_ok());
    }
}
