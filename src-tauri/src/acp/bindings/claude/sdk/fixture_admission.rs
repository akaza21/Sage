//! Desktop fixture admission: a `claude-sdk-*` replay case stands in for
//! the Claude Code CLI, so the real app drives the SDK binding without the
//! owner's Claude Code profile.

use std::ffi::OsString;

use super::*;

impl ClaudeSdkBindingRegistration {
    pub(crate) fn desktop_fixture(
        selection: crate::launch_profile::FixtureSelection,
    ) -> Result<Self, AdapterRuntimeError> {
        let binding = AdmittedClaudeSdkBinding::admit_fixture(selection)?;
        Ok(Self {
            state: Mutex::new(RegistrationState::Admitted(Arc::new(binding))),
            fixture_only: true,
        })
    }

    pub(crate) fn fixture_disabled() -> Self {
        Self {
            state: Mutex::new(RegistrationState::Unavailable(
                ClaudeSdkAdmissionUnavailable::Absent,
            )),
            fixture_only: true,
        }
    }
}

impl AdmittedClaudeSdkBinding {
    fn admit_fixture(
        selection: crate::launch_profile::FixtureSelection,
    ) -> Result<Self, AdapterRuntimeError> {
        if !crate::acp_protocol::native_cases::native_case_matches(&selection.case, "claude-sdk") {
            return Err(AdapterRuntimeError::LaunchUnavailable);
        }
        selection
            .revalidate()
            .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        let binding_digest: [u8; 32] = Sha256::new()
            .chain_update(b"sage:claude-code-sdk-fixture:v1:")
            .chain_update(selection.artifact_sha256)
            .chain_update(selection.case.as_bytes())
            .finalize()
            .into();
        Ok(Self {
            launcher: selection.artifact_path.clone(),
            executable: selection.artifact_path.clone(),
            version: CLAUDE_CODE_VERSION.into(),
            sha256: selection.artifact_sha256,
            sha256_hex: hex(&selection.artifact_sha256),
            byte_len: selection.artifact_bytes,
            binding_digest_hex: hex(&binding_digest),
            binding_digest,
            fixture: Some(selection),
        })
    }

    /// The replay child, confined to the fixture profile and project.
    pub(super) fn fixture_launch(
        &self,
        selection: &crate::launch_profile::FixtureSelection,
        epoch: u64,
        root: crate::project_registry::ProjectRootAuthority,
        session: Vec<OsString>,
    ) -> Result<LaunchSpec, AdapterRuntimeError> {
        fn unavailable<E>(_: E) -> AdapterRuntimeError {
            AdapterRuntimeError::LaunchUnavailable
        }
        if root.as_path() != selection.project_root {
            return Err(AdapterRuntimeError::LaunchUnavailable);
        }
        root.revalidate().map_err(unavailable)?;
        selection.revalidate().map_err(unavailable)?;
        // The case reads the session flags as Claude Code would: it opens,
        // resumes or forks that session's transcript in the fixture profile.
        let (binding_id, artifact, fixed_args) =
            selection.launch_metadata(self.binding_digest, session)?;
        let environment = BoundedEnvironment::new(
            [
                ("USERPROFILE", &selection.profile_root),
                ("CLAUDE_CONFIG_DIR", &selection.durable_dir),
                ("APPDATA", &selection.durable_dir),
                ("LOCALAPPDATA", &selection.durable_dir),
                ("TEMP", &selection.durable_dir),
                ("TMP", &selection.durable_dir),
            ]
            .into_iter()
            .map(|(key, path)| (OsString::from(key), path.as_os_str().to_owned()))
            .collect(),
        )
        .map_err(unavailable)?;
        selection.consume_one_shot().map_err(unavailable)?;
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
}
