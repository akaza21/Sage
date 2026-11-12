use super::*;
use crate::acp::{
    bindings::adapter::AdapterRuntimeError,
    process::{BoundedEnvironment, LaunchSpec, ProcessDeadlines, RequiredContainment, TreeLimits},
};

impl ClaudeBindingRegistration {
    pub(crate) fn desktop_fixture(
        selection: crate::launch_profile::FixtureSelection,
    ) -> Result<Self, ClaudeAdmissionUnavailable> {
        let binding = AdmittedClaudeBinding::admit_fixture(selection)?;
        Ok(Self {
            state: Mutex::new(RegistrationState::Admitted(Arc::new(binding))),
            fixture_only: true,
        })
    }
    pub(crate) fn fixture_disabled() -> Self {
        Self {
            state: Mutex::new(RegistrationState::Unavailable(
                ClaudeAdmissionUnavailable::AdapterAbsent,
            )),
            fixture_only: true,
        }
    }
}

impl AdmittedClaudeBinding {
    fn admit_fixture(
        selection: crate::launch_profile::FixtureSelection,
    ) -> Result<Self, ClaudeAdmissionUnavailable> {
        selection
            .revalidate()
            .map_err(|_| ClaudeAdmissionUnavailable::Drift)?;
        let artifact_sha256 = selection
            .artifact_sha256
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let binding_digest: [u8; 32] = Sha256::new()
            .chain_update(b"sage:nc1:claude-fixture:v1")
            .chain_update(selection.profile_root.as_os_str().as_encoded_bytes())
            .chain_update(selection.artifact_sha256)
            .chain_update(selection.case.as_bytes())
            .finalize()
            .into();
        let launch_environment = [
            ("USERPROFILE", &selection.profile_root),
            ("CLAUDE_CONFIG_DIR", &selection.durable_dir),
            ("APPDATA", &selection.durable_dir),
            ("LOCALAPPDATA", &selection.durable_dir),
            ("TEMP", &selection.durable_dir),
            ("TMP", &selection.durable_dir),
        ]
        .into_iter()
        .map(|(key, path)| (OsString::from(key), path.as_os_str().to_owned()))
        .collect();
        Ok(Self {
            candidate: super::super::AdmittedCandidate {
                adapter_version: "0.76.0".into(),
                characterisation:
                    crate::acp::bindings::characterisation::Characterisation::Characterised,
                artifact_path: selection.artifact_path.clone(),
                runtime_path: selection.artifact_path.clone(),
                artifact_sha256: artifact_sha256.clone(),
                runtime_sha256: artifact_sha256,
            },
            binding_digest,
            binding_digest_hex: binding_digest
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
            launch_environment,
            system_root: system_windows_directory()?,
            fixture: Some(selection),
        })
    }

    pub(crate) fn fixture_launch(
        &self,
        epoch: u64,
        root: crate::project_registry::ProjectRootAuthority,
        mcp: &crate::acp::mcp::McpAdmission,
    ) -> Result<LaunchSpec, AdapterRuntimeError> {
        let selection = self
            .fixture
            .as_ref()
            .ok_or(AdapterRuntimeError::LaunchUnavailable)?;
        if !mcp.is_empty() || root.as_path() != selection.project_root {
            return Err(AdapterRuntimeError::LaunchUnavailable);
        }
        root.revalidate()
            .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        let (binding_id, artifact, fixed_args) =
            selection.launch_metadata(self.binding_digest, Vec::new())?;
        let environment = BoundedEnvironment::new(self.launch_environment.clone())
            .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        selection
            .consume_one_shot()
            .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        Ok(LaunchSpec {
            binding_id,
            runtime_epoch: epoch,
            artifact,
            fixed_args,
            environment,
            project_root: root,
            required_guarantee: RequiredContainment::EnforcedTree,
            limits: TreeLimits::ade_0006a(),
            deadlines: ProcessDeadlines::ade_0006a(),
            absolute_cleanup_deadline: None,
            #[cfg(test)]
            test_after_artifact_verified: None,
        })
    }

    pub(crate) fn is_fixture(&self) -> bool {
        self.fixture.is_some()
    }
}
