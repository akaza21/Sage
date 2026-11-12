//! The Claude session a launch opens, and the CLI arguments that open it:
//! a new session under Sage's id, a resume of the stored one, cut before a
//! Turn the user undid, or a fork's first launch, which makes its copy.

use std::{ffi::OsString, path::Path};

use super::{
    admission::SDK_ARGS,
    fork::{Fork, ForkCopy},
    transcript,
};
use crate::acp::runtime::ActivationMode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionLaunch {
    pub(crate) session_id: String,
    pub(crate) resume: bool,
    pub(crate) model: Option<String>,
    /// The SDK message id of the undone prompt the resumed thread is cut
    /// before.
    pub(crate) rewind_before: Option<String>,
}

impl SessionLaunch {
    /// The launch an activation asks for; a `Load` has none, since Claude
    /// Code has no session load.
    pub(super) fn from_mode(mode: &ActivationMode) -> Option<Self> {
        let (session_id, resume, model, rewind_before) = match mode {
            ActivationMode::New { model, .. } => {
                (uuid::Uuid::new_v4().to_string(), false, model, None)
            }
            ActivationMode::Resume {
                external_session_id,
                model,
                rewind_before,
                ..
            } => (
                external_session_id.as_str().to_owned(),
                true,
                model,
                rewind_before
                    .as_ref()
                    .and_then(|turn| super::bridge::message_uuid(turn.as_str())),
            ),
            ActivationMode::Load { .. } => return None,
        };
        Some(Self {
            session_id,
            resume,
            model: model.as_ref().map(|model| model.as_str().to_owned()),
            rewind_before,
        })
    }

    pub(super) fn args(&self, config: Option<&Path>) -> Vec<OsString> {
        let mut args: Vec<OsString> = SDK_ARGS.iter().map(OsString::from).collect();
        args.extend(self.session_args(config));
        args
    }

    /// The arguments that pick the session and model. `config` is Claude
    /// Code's profile, where a fork's copy and a session's transcript are
    /// looked up, once per launch.
    pub(super) fn session_args(&self, config: Option<&Path>) -> Vec<OsString> {
        let mut args: Vec<OsString> = if self.resume {
            self.resume_args(config)
        } else {
            vec!["--session-id".into(), self.session_id.clone().into()]
        };
        if let Some(model) = self.model.as_deref().filter(|model| *model != "default") {
            args.extend(["--model".into(), model.into()]);
        }
        args
    }

    fn resume_args(&self, config: Option<&Path>) -> Vec<OsString> {
        let fork = Fork::parse(&self.session_id);
        // A fork's copy is looked up to know how to open it; a session's
        // own transcript only when there is a cut to find in it.
        let (session, written) = match (&fork, config) {
            (Some(fork), Some(config)) => match fork.copy(config) {
                ForkCopy::Pending => return fork.copy_args(),
                ForkCopy::Written(path) => (fork.session.as_str(), Some(path)),
                ForkCopy::Unknown => (fork.session.as_str(), None),
            },
            (Some(fork), None) => (fork.session.as_str(), None),
            (None, _) => (self.session_id.as_str(), None),
        };
        let mut args: Vec<OsString> = vec!["--resume".into(), session.into()];
        let Some(prompt) = &self.rewind_before else {
            return args;
        };
        let path = written.or_else(|| {
            fork.is_none()
                .then_some(config)
                .flatten()
                .and_then(|config| transcript::transcript(config, session))
        });
        match path.and_then(|path| transcript::cut_at(&path, prompt)) {
            Some(at) => args.extend(["--resume-session-at".into(), at.into()]),
            // The session resumes as it is, and the undone Turn stays in
            // Claude's context: refusing the launch would strand the Chat.
            None => tracing::warn!(
                session,
                "no reply found to cut the resumed thread before an undone turn"
            ),
        }
        args
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_sessions_take_sages_id_and_resumes_name_theirs() {
        let mut launch = SessionLaunch {
            session_id: "s-1".into(),
            resume: false,
            model: Some("opus".into()),
            rewind_before: None,
        };
        let args = launch.args(None);
        let tail: Vec<_> = args[SDK_ARGS.len()..]
            .iter()
            .map(|a| a.to_str().unwrap())
            .collect();
        assert_eq!(tail, ["--session-id", "s-1", "--model", "opus"]);
        launch.resume = true;
        launch.model = None;
        let args = launch.args(None);
        assert_eq!(
            args[SDK_ARGS.len()..],
            [OsString::from("--resume"), "s-1".into()]
        );
    }

    #[test]
    fn a_resume_after_an_undo_is_cut_at_the_reply_the_undone_prompt_followed() {
        let id = |n: u8| format!("00000000-0000-4000-8000-0000000000{n:02}");
        let (session, fork) = (id(90), id(91));
        let entry = |kind: &str, n: u8, parent: Option<u8>| {
            serde_json::json!({"type":kind,"uuid":id(n),"parentUuid":parent.map(id),"isSidechain":false})
                .to_string()
        };
        let mut sidechain: serde_json::Value =
            serde_json::from_str(&entry("assistant", 4, Some(3))).unwrap();
        sidechain["isSidechain"] = true.into();
        // A system entry sits between the first reply and the next prompt,
        // a subagent replies in between, and the third prompt was sent
        // after an earlier cut, so it follows the first reply again.
        let lines = [
            entry("user", 1, None),
            entry("assistant", 2, Some(1)),
            entry("system", 3, Some(2)),
            sidechain.to_string(),
            entry("user", 5, Some(3)),
            entry("assistant", 6, Some(5)),
            entry("user", 7, Some(2)),
            entry("assistant", 8, Some(7)),
        ]
        .join("\n");
        let profile =
            std::env::temp_dir().join(format!("sage-undo-profile-{}", uuid::Uuid::new_v4()));
        let project = profile.join("projects").join("-w");
        std::fs::create_dir_all(&project).unwrap();
        for name in [&session, &fork] {
            std::fs::write(project.join(format!("{name}.jsonl")), &lines).unwrap();
        }
        let resumed = |session_id: String, prompt: u8, config: &std::path::Path| {
            SessionLaunch {
                session_id,
                resume: true,
                model: None,
                rewind_before: Some(id(prompt)),
            }
            .session_args(Some(config))
        };
        let cut = |session: &str, at: u8| {
            ["--resume", session, "--resume-session-at", &id(at)].map(OsString::from)
        };
        assert_eq!(resumed(session.clone(), 5, &profile), cut(&session, 2));
        assert_eq!(resumed(session.clone(), 7, &profile), cut(&session, 2));
        // A fork whose copy exists resumes the copy, cut the same way.
        let fork_id = format!("{fork}~{session}~{}", id(2));
        assert_eq!(resumed(fork_id, 7, &profile), cut(&fork, 2));
        // No reply before the first prompt, and no readable transcript,
        // resume the session as it is.
        let plain = ["--resume", session.as_str()].map(OsString::from);
        assert_eq!(resumed(session.clone(), 1, &profile), plain);
        let _ = std::fs::remove_dir_all(&profile);
        assert_eq!(resumed(session.clone(), 5, &profile), plain);
    }

    #[test]
    fn a_fork_launches_from_its_source_until_without_a_profile_it_resumes_itself() {
        let session = "0b7d6c8e-3c1a-4f7e-9a51-2f6d8c1e4b90";
        let source = "1c8e7d9f-4d2b-4a8f-8b62-3a7e9d2f5ca1";
        let at = "2d9f8e0a-5e3c-4b9a-9c73-4b8fae306db2";
        let launch = SessionLaunch {
            session_id: format!("{session}~{source}~{at}"),
            resume: true,
            model: None,
            rewind_before: None,
        };
        let empty = std::env::temp_dir().join(format!("sage-empty-profile-{session}"));
        std::fs::create_dir_all(empty.join("projects")).unwrap();
        assert_eq!(
            launch.args(Some(&empty))[SDK_ARGS.len()..],
            [
                "--resume",
                source,
                "--resume-session-at",
                at,
                "--fork-session",
                "--session-id",
                session
            ]
            .map(OsString::from)
        );
        let _ = std::fs::remove_dir_all(&empty);
        // A profile Sage cannot read, or none, resumes the copy.
        for config in [Some(empty.as_path()), None] {
            assert_eq!(
                launch.args(config)[SDK_ARGS.len()..],
                ["--resume", session].map(OsString::from)
            );
        }
    }
}
