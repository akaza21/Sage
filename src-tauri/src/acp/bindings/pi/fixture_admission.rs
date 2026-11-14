use std::{ffi::OsString, sync::Arc};

use sha2::{Digest, Sha256};

use crate::acp::{
    bindings::adapter::AdapterRuntimeError,
    process::{BoundedEnvironment, LaunchSpec, ProcessDeadlines, RequiredContainment, TreeLimits},
};

pub(crate) const PI_FIXTURE_BINDING_ID: &str = "pi-rpc";

pub(crate) struct PiFixtureBinding {
    selection: crate::launch_profile::FixtureSelection,
    digest: [u8; 32],
    digest_hex: String,
    artifact_hex: String,
}

impl PiFixtureBinding {
    pub(crate) fn admit(
        selection: crate::launch_profile::FixtureSelection,
    ) -> Result<Arc<Self>, AdapterRuntimeError> {
        if !crate::acp_protocol::native_cases::native_case_matches(&selection.case, "pi") {
            return Err(AdapterRuntimeError::LaunchUnavailable);
        }
        selection
            .revalidate()
            .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        let digest: [u8; 32] = Sha256::new()
            .chain_update(b"sage:nc1b:pi-fixture:v1")
            .chain_update(super::PI_SOURCE_COMMIT)
            .chain_update(selection.artifact_sha256)
            .chain_update(selection.case.as_bytes())
            .finalize()
            .into();
        let hex = |bytes: &[u8]| {
            bytes
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        };
        Ok(Arc::new(Self {
            digest,
            digest_hex: hex(&digest),
            artifact_hex: hex(&selection.artifact_sha256),
            selection,
        }))
    }

    pub(crate) fn revalidate(&self) -> Result<(), AdapterRuntimeError> {
        self.selection
            .revalidate()
            .map_err(|_| AdapterRuntimeError::LaunchUnavailable)
    }

    pub(crate) fn bridge_installed(&self) -> bool {
        crate::acp_protocol::native_cases::native_case_uses_pi_bridge(&self.selection.case)
    }

    pub(crate) fn binding_digest(&self) -> [u8; 32] {
        self.digest
    }
    pub(crate) fn binding_digest_hex(&self) -> &str {
        &self.digest_hex
    }
    pub(crate) fn artifact_digest(&self) -> &str {
        &self.artifact_hex
    }

    pub(crate) fn launch(
        &self,
        epoch: u64,
        root: crate::project_registry::ProjectRootAuthority,
        mcp: &crate::acp::mcp::McpAdmission,
    ) -> Result<LaunchSpec, AdapterRuntimeError> {
        if !mcp.is_empty() || root.as_path() != self.selection.project_root {
            return Err(AdapterRuntimeError::LaunchUnavailable);
        }
        root.revalidate()
            .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        self.revalidate()?;
        let (binding_id, artifact, fixed_args) =
            self.selection.launch_metadata(self.digest, Vec::new())?;
        let environment = BoundedEnvironment::new(
            [
                ("USERPROFILE", &self.selection.profile_root),
                ("PI_CODING_AGENT_DIR", &self.selection.durable_dir),
                ("APPDATA", &self.selection.durable_dir),
                ("LOCALAPPDATA", &self.selection.durable_dir),
                ("TEMP", &self.selection.durable_dir),
                ("TMP", &self.selection.durable_dir),
            ]
            .into_iter()
            .map(|(key, path)| (OsString::from(key), path.as_os_str().to_owned()))
            .collect(),
        )
        .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        self.selection
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
}
