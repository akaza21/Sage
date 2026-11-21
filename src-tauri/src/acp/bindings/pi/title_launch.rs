//! Reuse the main process's environment and auth/model configuration; fail
//! closed if those files change while the main session holds cached authority.
use crate::acp::{bindings::adapter::AdapterRuntimeError, process::LaunchSpec};
use std::{path::PathBuf, process::Command};

pub(super) struct TitleLaunch {
    command: Command,
    configuration: Vec<(PathBuf, Option<String>)>,
}

impl TitleLaunch {
    #[cfg(all(test, feature = "acp-fixture"))]
    pub(super) fn fixture(command: Command) -> Self {
        Self {
            command,
            configuration: Vec::new(),
        }
    }

    pub(super) fn capture(spec: &LaunchSpec) -> Option<Self> {
        let environment = spec.environment.as_slice();
        let home = environment
            .iter()
            .find(|(key, _)| key == if cfg!(windows) { "USERPROFILE" } else { "HOME" })?
            .1
            .clone();
        let agent = PathBuf::from(home).join(".pi").join("agent");
        let paths = [
            agent.join("auth.json"),
            agent.join("models.json"),
            agent.join("settings.json"),
            spec.project_root.as_path().join(".pi/settings.json"),
        ];
        let configuration = paths
            .into_iter()
            .map(|path| {
                let digest = fingerprint(&path)?;
                Some((path, digest))
            })
            .collect::<Option<Vec<_>>>()?;
        let mut command = Command::new(&spec.artifact.canonical_absolute_path);
        command
            .arg(spec.fixed_args.as_slice().first()?)
            .args(super::title_request::TITLE_ARGS)
            .env_clear()
            .envs(environment.iter().cloned())
            .current_dir(spec.project_root.as_path());
        Some(Self {
            command,
            configuration,
        })
    }

    pub(super) fn command(
        &self,
        model: &super::ModelSummary,
    ) -> Result<Command, AdapterRuntimeError> {
        for (path, expected) in &self.configuration {
            if fingerprint(path).as_ref() != Some(expected) {
                return Err(AdapterRuntimeError::LaunchUnavailable);
            }
        }
        if model.provider.is_empty() || model.id.is_empty() {
            return Err(AdapterRuntimeError::LaunchUnavailable);
        }
        let mut command = Command::new(self.command.get_program());
        command.args(self.command.get_args()).env_clear();
        for (key, value) in self.command.get_envs() {
            if let Some(value) = value {
                command.env(key, value);
            }
        }
        if let Some(directory) = self.command.get_current_dir() {
            command.current_dir(directory);
        }
        command.args(["--provider", &model.provider, "--model", &model.id]);
        Ok(command)
    }
}

fn fingerprint(path: &PathBuf) -> Option<Option<String>> {
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.len() <= 256 * 1024 => {
            crate::harness_discovery::sha256_file(path).map(Some)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(None),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
