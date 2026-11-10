//! Forking a Claude Code conversation.
//!
//! Claude Code has no control request that forks a running session. It
//! forks when launched to resume one with `--fork-session`, copying the
//! transcript under a new id, and `--resume-session-at` cuts the copy at an
//! assistant message. Sage forks lazily that way: the fork's session id
//! names the new Claude session, the source session and the source's last
//! assistant message at the time of the fork. The fork chat's first launch
//! makes the copy; once Claude Code has written it, later launches resume
//! it like any session. Claude Code writes the copy only with the first
//! message, so a launch that ends sooner forks again next time.

use std::{
    ffi::OsString,
    io::BufRead,
    path::{Path, PathBuf},
};

const SEPARATOR: char = '~';

/// A fork of `source` cut after its assistant message `at`, to become the
/// Claude session `session`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Fork {
    pub(super) session: String,
    pub(super) source: String,
    pub(super) at: String,
}

impl Fork {
    /// The fork's session id as Sage stores it.
    pub(super) fn id(&self) -> String {
        format!(
            "{}{SEPARATOR}{}{SEPARATOR}{}",
            self.session, self.source, self.at
        )
    }

    /// A stored session id that names a fork.
    pub(super) fn parse(id: &str) -> Option<Self> {
        let mut parts = id.split(SEPARATOR);
        let (session, source, at) = (parts.next()?, parts.next()?, parts.next()?);
        if parts.next().is_some() || ![session, source, at].into_iter().all(is_uuid) {
            return None;
        }
        Some(Self {
            session: session.to_owned(),
            source: source.to_owned(),
            at: at.to_owned(),
        })
    }

    /// A fork through the latest assistant message of `source` (a stored
    /// session id, itself perhaps a fork), when its transcript has one.
    pub(super) fn of(source: &str, config: &Path) -> Option<Self> {
        let source = claude_session(source).to_owned();
        let at = last_reply(&transcript(config, &source)?)?;
        Some(Self {
            session: uuid::Uuid::new_v4().to_string(),
            source,
            at,
        })
    }

    /// The launch arguments that open this fork.
    pub(super) fn launch_args(&self, config: &Path) -> Vec<OsString> {
        if transcript(config, &self.session).is_some() {
            return vec!["--resume".into(), self.session.clone().into()];
        }
        [
            "--resume",
            &self.source,
            "--resume-session-at",
            &self.at,
            "--fork-session",
            "--session-id",
            &self.session,
        ]
        .map(OsString::from)
        .into()
    }
}

/// The Claude session a stored session id names.
pub(super) fn claude_session(id: &str) -> &str {
    id.split(SEPARATOR).next().unwrap_or(id)
}

fn is_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|parsed| parsed.hyphenated().to_string() == value)
}

/// Claude Code's profile directory, as its launch environment names it.
pub(super) fn config_directory(environment: &[(OsString, OsString)]) -> Option<PathBuf> {
    let value = |key: &str| {
        environment
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| PathBuf::from(value))
    };
    value("CLAUDE_CONFIG_DIR")
        .or_else(|| value("HOME").map(|home| home.join(".claude")))
        .or_else(|| value("USERPROFILE").map(|home| home.join(".claude")))
}

/// A session's transcript: Claude Code keeps each project's sessions in
/// their own directory under `projects`.
fn transcript(config: &Path, session: &str) -> Option<PathBuf> {
    let name = format!("{session}.jsonl");
    std::fs::read_dir(config.join("projects"))
        .ok()?
        .filter_map(Result::ok)
        .map(|project| project.path().join(&name))
        .find(|path| path.is_file())
}

/// The id of the transcript's last assistant message in the main thread.
fn last_reply(path: &Path) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    let mut last = None;
    for line in std::io::BufReader::new(file).lines() {
        let Ok(line) = line else { break };
        // Only an assistant entry can carry the id this looks for.
        if !line.contains("\"assistant\"") {
            continue;
        }
        let Ok(entry) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if entry["type"] == "assistant"
            && entry["isSidechain"] != true
            && let Some(id) = entry["uuid"].as_str().filter(|id| is_uuid(id))
        {
            last = Some(id.to_owned());
        }
    }
    last
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: &str = "0b7d6c8e-3c1a-4f7e-9a51-2f6d8c1e4b90";
    const A: &str = "1c8e7d9f-4d2b-4a8f-8b62-3a7e9d2f5ca1";

    struct Profile(PathBuf);

    impl Profile {
        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Profile {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn profile(entries: &[serde_json::Value]) -> Profile {
        let config = Profile(
            std::env::temp_dir().join(format!("sage-claude-fork-{}", uuid::Uuid::new_v4())),
        );
        let project = config.path().join("projects").join("-w");
        std::fs::create_dir_all(&project).unwrap();
        let lines: Vec<String> = entries.iter().map(ToString::to_string).collect();
        std::fs::write(project.join(format!("{S}.jsonl")), lines.join("\n")).unwrap();
        config
    }

    #[test]
    fn a_fork_is_cut_at_the_sources_last_main_thread_reply_and_launches_once_as_a_copy() {
        let config = profile(&[
            serde_json::json!({"type":"user","uuid":"u1"}),
            serde_json::json!({"type":"assistant","uuid":A,"isSidechain":false}),
            serde_json::json!({"type":"assistant","uuid":"2d9f8e0a-5e3c-4b9a-9c73-4b8fae306db2","isSidechain":true}),
            serde_json::json!({"type":"summary","leafUuid":"x"}),
        ]);
        let fork = Fork::of(S, config.path()).unwrap();
        assert_eq!((fork.source.as_str(), fork.at.as_str()), (S, A));
        let id = fork.id();
        assert_eq!(Fork::parse(&id), Some(fork.clone()));
        assert_eq!(claude_session(&id), fork.session);
        assert_eq!(
            fork.launch_args(config.path()),
            [
                "--resume",
                S,
                "--resume-session-at",
                A,
                "--fork-session",
                "--session-id",
                &fork.session
            ]
        );
        // Once Claude Code wrote the copy, the fork resumes as itself.
        let project = config.path().join("projects").join("-w");
        std::fs::write(project.join(format!("{}.jsonl", fork.session)), "{}").unwrap();
        assert_eq!(fork.launch_args(config.path()), ["--resume", &fork.session]);
        // A fork of the fork starts from the copy's own transcript.
        assert_eq!(
            Fork::of(&id, config.path()).map(|again| again.source),
            None,
            "the copy holds no reply yet"
        );
    }

    #[test]
    fn only_sessions_with_a_reply_fork_and_only_well_formed_ids_parse() {
        let config = profile(&[serde_json::json!({"type":"user","uuid":"u1"})]);
        assert_eq!(Fork::of(S, config.path()), None);
        assert_eq!(Fork::of(A, config.path()), None, "no transcript");
        assert_eq!(Fork::parse(S), None);
        assert_eq!(Fork::parse(&format!("{S}~{A}")), None);
        assert_eq!(Fork::parse(&format!("{S}~{A}~{A}~{A}")), None);
        assert_eq!(Fork::parse(&format!("{S}~{A}~--fork-session")), None);
        assert_eq!(claude_session(S), S);
    }

    #[test]
    fn the_profile_is_the_configured_one_or_the_home_default() {
        let env = |pairs: &[(&str, &str)]| -> Vec<(OsString, OsString)> {
            pairs
                .iter()
                .map(|(k, v)| ((*k).into(), (*v).into()))
                .collect()
        };
        assert_eq!(
            config_directory(&env(&[("HOME", "/h"), ("CLAUDE_CONFIG_DIR", "/c")])),
            Some(PathBuf::from("/c"))
        );
        assert_eq!(
            config_directory(&env(&[("HOME", "/h")])),
            Some(PathBuf::from("/h/.claude"))
        );
        assert_eq!(config_directory(&env(&[("PATH", "/bin")])), None);
    }
}
