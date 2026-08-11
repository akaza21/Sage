use std::ffi::OsString;

use uuid::Uuid;

use crate::acp::{
    bindings::adapter::AdapterRuntimeError,
    process::{
        ArtifactExpectation, BoundedEnvironment, BoundedOsArgs, LaunchSpec, ProcessDeadlines,
        RequiredContainment, TreeLimits,
    },
};
use crate::project_registry::ProjectRootAuthority;

use super::CodexRuntime;

impl CodexRuntime {
    /// Freezes the supervisor contract from the authority retained by this
    /// exact runtime. Metadata-only adapters cannot construct this value.
    pub(crate) fn launch_spec(
        &self,
        project_root: ProjectRootAuthority,
    ) -> Result<LaunchSpec, AdapterRuntimeError> {
        let binding = self
            .admission
            .as_ref()
            .ok_or(AdapterRuntimeError::LaunchUnavailable)?;
        binding
            .revalidate()
            .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        let runtime_root = self
            .project_root
            .as_ref()
            .ok_or(AdapterRuntimeError::LaunchUnavailable)?;
        if runtime_root.project_id != *project_root.project_id()
            || runtime_root.generation != project_root.generation()
            || runtime_root.root_id != *project_root.root_id()
        {
            return Err(AdapterRuntimeError::LaunchUnavailable);
        }
        project_root
            .revalidate()
            .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;

        let entrypoint = binding.entrypoint();
        let environment = binding
            .environment()
            .entries()
            .iter()
            .map(|(key, value)| (OsString::from(key), OsString::from(value)))
            .collect();
        Ok(LaunchSpec {
            binding_id: digest_identity(binding.binding_digest()),
            runtime_epoch: self.inner.runtime_epoch().get(),
            artifact: ArtifactExpectation::new(
                digest_identity(entrypoint.sha256),
                entrypoint.path.clone(),
                entrypoint.sha256,
                entrypoint.byte_len,
            )
            .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?,
            fixed_args: BoundedOsArgs::new(vec![OsString::from("app-server")])
                .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?,
            environment: BoundedEnvironment::new(environment)
                .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?,
            project_root,
            required_guarantee: RequiredContainment::EnforcedTree,
            limits: TreeLimits::ade_0006a(),
            deadlines: ProcessDeadlines::ade_0006a(),
            #[cfg(any(test, all(windows, feature = "acp-fixture")))]
            absolute_cleanup_deadline: None,
            #[cfg(all(test, windows))]
            test_after_artifact_verified: None,
        })
    }
}

fn digest_identity(digest: [u8; 32]) -> Uuid {
    let mut identity = [0_u8; 16];
    identity.copy_from_slice(&digest[..16]);
    Uuid::from_bytes(identity)
}
