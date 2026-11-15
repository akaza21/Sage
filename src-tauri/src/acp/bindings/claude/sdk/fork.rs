//! Claude Code has no control request that forks a running session. It
//! forks when launched to resume one with `--fork-session`, copying the
//! transcript under a new id, and `--resume-session-at` cuts the copy at an
//! assistant message. Claude Code writes the copy only with the first
//! message, so a launch that ends sooner forks again next time.

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use super::transcript::{is_uuid, last_reply, transcript};

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
    pub(super) fn id(&self) -> String {
        format!(
            "{}{SEPARATOR}{}{SEPARATOR}{}",
            self.session, self.source, self.at
        )
    }

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

    pub(super) fn of(source: &str, config: &Path) -> Option<Self> {
        let source = claude_session(source).to_owned();
        let at = last_reply(&transcript(config, &source)?)?;
        Some(Self {
            session: uuid::Uuid::new_v4().to_string(),
            source,
            at,
        })
    }

    pub(super) fn copy(&self, config: &Path) -> ForkCopy {
        if std::fs::read_dir(config.join("projects")).is_err() {
            return ForkCopy::Unknown;
        }
        transcript(config, &self.session).map_or(ForkCopy::Pending, ForkCopy::Written)
    }

    pub(super) fn copy_args(&self) -> Vec<OsString> {
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

    #[cfg(test)]
    fn launch_args(&self, config: &Path) -> Vec<OsString> {
        match self.copy(config) {
            ForkCopy::Pending => self.copy_args(),
            _ => vec!["--resume".into(), self.session.clone().into()],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ForkCopy {
    /// Not written yet: the next launch makes it.
    Pending,
    Written(PathBuf),
    /// The profile cannot be read. It counts as holding the copy: resuming
    /// a missing session fails as plainly, and a repeated fork would be
    /// refused on every later launch.
    Unknown,
}

pub(super) fn claude_session(id: &str) -> &str {
    id.split(SEPARATOR).next().unwrap_or(id)
}

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
        // Claude Code reads a relative one from the project, not from Sage.
        .filter(|config| config.is_absolute())
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
        let project = config.path().join("projects").join("-w");
        std::fs::write(project.join(format!("{}.jsonl", fork.session)), "{}").unwrap();
        assert_eq!(fork.launch_args(config.path()), ["--resume", &fork.session]);
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
        let root = std::env::temp_dir();
        let (home, custom) = (root.join("h"), root.join("c"));
        let env = |pairs: &[(&str, &Path)]| -> Vec<(OsString, OsString)> {
            pairs
                .iter()
                .map(|(key, value)| ((*key).into(), value.as_os_str().to_owned()))
                .collect()
        };
        assert_eq!(
            config_directory(&env(&[("HOME", &home), ("CLAUDE_CONFIG_DIR", &custom)])),
            Some(custom.clone())
        );
        assert_eq!(
            config_directory(&env(&[("HOME", &home)])),
            Some(home.join(".claude"))
        );
        assert_eq!(config_directory(&env(&[("PATH", &home)])), None);
        assert_eq!(
            config_directory(&env(&[
                ("HOME", &home),
                ("CLAUDE_CONFIG_DIR", Path::new("rel"))
            ])),
            None,
            "Claude Code reads a relative one from elsewhere"
        );
    }
}
