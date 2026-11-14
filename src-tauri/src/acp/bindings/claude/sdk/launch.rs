use std::{ffi::OsString, path::Path};

use super::{
    admission::SDK_ARGS,
    fork::{Fork, ForkCopy},
    transcript,
};
use crate::acp::{bindings::adapter::AdapterRuntimeError, runtime::ActivationMode};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionLaunch {
    pub(crate) session_id: String,
    pub(crate) resume: bool,
    pub(crate) model: Option<String>,
    pub(crate) rewind_at: Option<String>,
}

impl SessionLaunch {
    /// A `Load` has none, since Claude Code has no session load.
    pub(super) fn from_mode(mode: &ActivationMode) -> Option<Self> {
        let (session_id, resume, model, rewind_at) = match mode {
            ActivationMode::New { model, .. } => {
                (uuid::Uuid::new_v4().to_string(), false, model, None)
            }
            ActivationMode::Resume {
                external_session_id,
                model,
                rewind_at,
                ..
            } => (
                external_session_id.as_str().to_owned(),
                true,
                model,
                rewind_at.clone(),
            ),
            ActivationMode::Load { .. } => return None,
        };
        Some(Self {
            session_id,
            resume,
            model: model.as_ref().map(|model| model.as_str().to_owned()),
            rewind_at,
        })
    }

    pub(super) fn args(&self, config: Option<&Path>) -> Result<Vec<OsString>, AdapterRuntimeError> {
        let mut args: Vec<OsString> = SDK_ARGS.iter().map(OsString::from).collect();
        args.extend(self.session_args(config)?);
        Ok(args)
    }

    pub(super) fn session_args(
        &self,
        config: Option<&Path>,
    ) -> Result<Vec<OsString>, AdapterRuntimeError> {
        let mut args: Vec<OsString> = if self.resume {
            self.resume_args(config)?
        } else {
            vec!["--session-id".into(), self.session_id.clone().into()]
        };
        if let Some(model) = self.model.as_deref().filter(|model| *model != "default") {
            args.extend(["--model".into(), model.into()]);
        }
        Ok(args)
    }

    fn resume_args(&self, config: Option<&Path>) -> Result<Vec<OsString>, AdapterRuntimeError> {
        let fork = Fork::parse(&self.session_id);
        let (session, written) = match (&fork, config) {
            (Some(fork), Some(config)) => match fork.copy(config) {
                ForkCopy::Pending if self.rewind_at.is_none() => return Ok(fork.copy_args()),
                ForkCopy::Pending => return Err(AdapterRuntimeError::RewindUnavailable),
                ForkCopy::Written(path) => (fork.session.as_str(), Some(path)),
                ForkCopy::Unknown => (fork.session.as_str(), None),
            },
            (Some(fork), None) => (fork.session.as_str(), None),
            (None, _) => (self.session_id.as_str(), None),
        };
        let mut args: Vec<OsString> = vec!["--resume".into(), session.into()];
        let Some(at) = &self.rewind_at else {
            return Ok(args);
        };
        let path = written.or_else(|| {
            fork.is_none()
                .then_some(config)
                .flatten()
                .and_then(|config| transcript::transcript(config, session))
        });
        let path = path.ok_or(AdapterRuntimeError::RewindUnavailable)?;
        transcript::validate_cut(&path, at).map_err(|_| AdapterRuntimeError::RewindUnavailable)?;
        args.extend(["--resume-session-at".into(), at.into()]);
        Ok(args)
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
            rewind_at: None,
        };
        let args = launch.args(None).unwrap();
        let tail: Vec<_> = args[SDK_ARGS.len()..]
            .iter()
            .map(|a| a.to_str().unwrap())
            .collect();
        assert_eq!(tail, ["--session-id", "s-1", "--model", "opus"]);
        launch.resume = true;
        launch.model = None;
        let args = launch.args(None).unwrap();
        assert_eq!(
            args[SDK_ARGS.len()..],
            [OsString::from("--resume"), "s-1".into()]
        );
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
            rewind_at: None,
        };
        let empty = std::env::temp_dir().join(format!("sage-empty-profile-{session}"));
        std::fs::create_dir_all(empty.join("projects")).unwrap();
        assert_eq!(
            launch.args(Some(&empty)).unwrap()[SDK_ARGS.len()..],
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
        for config in [Some(empty.as_path()), None] {
            assert_eq!(
                launch.args(config).unwrap()[SDK_ARGS.len()..],
                ["--resume", session].map(OsString::from)
            );
        }
    }
}
