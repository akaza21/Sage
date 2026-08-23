use std::ffi::OsString;

use uuid::Uuid;

use super::CodexRuntime;
use crate::acp::{
    bindings::adapter::AdapterRuntimeError,
    process::{
        ArtifactExpectation, BoundedEnvironment, BoundedOsArgs, LaunchSpec, ProcessDeadlines,
        RequiredContainment, TreeLimits,
    },
};

impl CodexRuntime {
    /// Freezes the supervisor contract from the authority retained by this
    /// exact runtime. Metadata-only adapters cannot construct this value.
    pub(crate) fn launch_spec(&mut self) -> Result<LaunchSpec, AdapterRuntimeError> {
        self.launch_authority.revalidate()?;
        let binding = self.launch_authority.binding()?;
        let entrypoint = binding.entrypoint();
        #[cfg(all(test, windows))]
        let entrypoint = binding.journey_test_entrypoint().unwrap_or(entrypoint);
        let environment = binding
            .environment()
            .entries()
            .iter()
            .map(|(key, value)| (OsString::from(key), OsString::from(value)))
            .collect();
        let binding_id = digest_identity(binding.binding_digest());
        let runtime_epoch = self.inner.runtime_epoch().get();
        let artifact = ArtifactExpectation::new(
            digest_identity(entrypoint.sha256),
            entrypoint.path.clone(),
            entrypoint.sha256,
            entrypoint.byte_len,
        )
        .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        #[cfg(all(test, windows))]
        let fixed_args = if binding.journey_test_entrypoint().is_some() {
            vec![
                OsString::from("--case"),
                OsString::from("characterization-turn-success"),
                OsString::from("--seed"),
                OsString::from("0"),
            ]
        } else {
            vec![OsString::from("app-server")]
        };
        #[cfg(not(all(test, windows)))]
        let fixed_args = vec![OsString::from("app-server")];
        let fixed_args =
            BoundedOsArgs::new(fixed_args).map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        let environment = BoundedEnvironment::new(environment)
            .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        let project_root = self.launch_authority.take_project_root()?;
        Ok(LaunchSpec {
            binding_id,
            runtime_epoch,
            artifact,
            fixed_args,
            environment,
            project_root,
            required_guarantee: RequiredContainment::EnforcedTree,
            limits: TreeLimits::ade_0006a(),
            deadlines: ProcessDeadlines::ade_0006a(),
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
