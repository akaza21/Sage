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
        #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
        if let Some(expected_root) = binding.desktop_fixture_project_root()
            && self.launch_authority.project_root_path()? != expected_root
        {
            return Err(AdapterRuntimeError::LaunchUnavailable);
        }
        #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
        if binding.is_desktop_fixture() && !self.mcp.is_empty() {
            return Err(AdapterRuntimeError::LaunchUnavailable);
        }
        let entrypoint = binding.entrypoint();
        #[cfg(all(test, windows))]
        let entrypoint = binding.journey_test_entrypoint().unwrap_or(entrypoint);
        let mut environment: Vec<_> = binding
            .launch_environment()
            .iter()
            .map(|(key, value)| (OsString::from(key), OsString::from(value)))
            .collect();
        environment.extend(
            self.mcp
                .codex_environment()
                .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?
                .into_iter()
                .map(|(key, value)| (OsString::from(key), OsString::from(value))),
        );
        let binding_id = digest_identity(binding.binding_digest());
        let runtime_epoch = self.inner.runtime_epoch().get();
        #[cfg(windows)]
        let artifact = if is_batch_launcher(&entrypoint.path) {
            ArtifactExpectation::windows_batch_launcher(
                digest_identity(entrypoint.sha256),
                entrypoint.path.clone(),
                entrypoint.sha256,
                entrypoint.byte_len,
            )
        } else {
            ArtifactExpectation::new(
                digest_identity(entrypoint.sha256),
                entrypoint.path.clone(),
                entrypoint.sha256,
                entrypoint.byte_len,
            )
        };
        #[cfg(not(windows))]
        let artifact = ArtifactExpectation::new(
            digest_identity(entrypoint.sha256),
            entrypoint.path.clone(),
            entrypoint.sha256,
            entrypoint.byte_len,
        );
        let artifact = artifact.map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        #[cfg(all(test, windows))]
        let fixed_args = if binding.journey_test_entrypoint().is_some() {
            let case_id = binding
                .next_journey_test_case()
                .ok_or(AdapterRuntimeError::LaunchUnavailable)?;
            vec![
                OsString::from("--case"),
                OsString::from(case_id),
                OsString::from("--seed"),
                OsString::from("0"),
            ]
        } else {
            vec![OsString::from("app-server")]
        };
        #[cfg(not(all(test, windows)))]
        let fixed_args = vec![OsString::from("app-server")];
        #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
        let fixed_args = if binding.is_desktop_fixture() {
            vec![
                OsString::from("--case"),
                OsString::from(
                    binding
                        .desktop_fixture_case()
                        .ok_or(AdapterRuntimeError::LaunchUnavailable)?,
                ),
                OsString::from("--seed"),
                OsString::from("0"),
            ]
        } else {
            fixed_args
        };
        let fixed_args =
            BoundedOsArgs::new(fixed_args).map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        let environment = BoundedEnvironment::new(environment)
            .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
        if binding.is_desktop_fixture() {
            binding
                .consume_desktop_fixture()
                .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        }
        let project_root = self.launch_authority.take_project_root()?;
        Ok(LaunchSpec {
            binding_id,
            runtime_epoch,
            artifact,
            fixed_args,
            environment,
            project_root,
            required_guarantee: if cfg!(windows) {
                RequiredContainment::EnforcedTree
            } else {
                // WSL is an explicitly admitted functional-beta host. Its
                // supervisor owns the root process only; it must never be
                // presented as Windows Job-object tree containment.
                RequiredContainment::TrustedCooperativeTree
            },
            limits: TreeLimits::ade_0006a(),
            deadlines: ProcessDeadlines::ade_0006a(),
            absolute_cleanup_deadline: None,
            #[cfg(all(test, windows))]
            test_after_artifact_verified: None,
        })
    }
}

#[cfg(windows)]
fn is_batch_launcher(path: &std::path::Path) -> bool {
    matches!(
        path.extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("cmd" | "bat")
    )
}

fn digest_identity(digest: [u8; 32]) -> Uuid {
    let mut identity = [0_u8; 16];
    identity.copy_from_slice(&digest[..16]);
    Uuid::from_bytes(identity)
}
