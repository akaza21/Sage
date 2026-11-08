//! Harness-neutral session actions. Each runs as an ordinary Turn on an idle
//! session whose negotiated capabilities admit it; a binding maps it to its
//! own native method.
use serde::Deserialize;

use super::{contract::RequiredText, intent::CapabilitySnapshotV1};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum ReviewTarget {
    UncommittedChanges {},
    BaseBranch {
        branch: RequiredText<256>,
    },
    Commit {
        sha: RequiredText<64>,
        title: Option<RequiredText<512>>,
    },
    Custom {
        instructions: RequiredText<16384>,
    },
}

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
        let review: SessionAction = serde_json::from_value(
            json!({"kind":"review","target":{"kind":"baseBranch","branch":"main"}}),
        )
        .unwrap();
        assert_eq!(review.prompt_text(), "/review main");
        assert_eq!(
            serde_json::from_value::<SessionAction>(json!({"kind":"compact"}))
                .unwrap()
                .prompt_text(),
            "/compact"
        );
        for invalid in [
            json!({"kind":"review","target":{"kind":"baseBranch","branch":""}}),
            json!({"kind":"review","target":{"kind":"commit","sha":"x".repeat(65)}}),
            json!({"kind":"review","target":{"kind":"uncommittedChanges","extra":1}}),
            json!({"kind":"compact","extra":1}),
            json!({"kind":"revert"}),
        ] {
            assert!(serde_json::from_value::<SessionAction>(invalid).is_err());
        }
    }
}
