use std::ffi::OsString;

use crate::acp::{
    bindings::{adapter::AdapterRuntimeError, runtime::LaunchableRuntime},
    process::{BoundedEnvironment, LaunchSpec},
};

use super::{
    super::{
        AcpLaunchAuthority, StandardAcpProfile,
        admission::StandardAcpLaunchExtension,
        environment::{extend_passthrough, launch_environment as baseline_environment},
        opencode_policy,
        profile::StandardAcpLaunchPolicy,
    },
    StandardAcpRuntime,
};

impl LaunchableRuntime for StandardAcpRuntime {
    fn launch_spec(&mut self) -> Result<LaunchSpec, AdapterRuntimeError> {
        let project_root = self
            .project_root
            .take()
            .ok_or(AdapterRuntimeError::LaunchUnavailable)?;
        let binding = self
            .source
            .refresh_binding()
            .ok_or(AdapterRuntimeError::LaunchUnavailable)?;
        if binding.binding_digest() != self.binding_digest
            || !std::ptr::eq(binding.profile(), self.profile)
        {
            return Err(AdapterRuntimeError::LaunchUnavailable);
        }
        let entries = launch_environment(self.profile, binding.extension(), |key| {
            std::env::var_os(key)
        })?;
        AcpLaunchAuthority::from_admitted_binding(
            &binding,
            self.runtime_epoch.get(),
            BoundedEnvironment::new(entries).map_err(|_| AdapterRuntimeError::LaunchUnavailable)?,
        )?
        .into_launch_spec(project_root)
    }
}

/// The child's environment: the platform baseline, the profile's and the
/// user's passthrough names, and any variable the profile's policy pins.
pub(super) fn launch_environment(
    profile: &StandardAcpProfile,
    extension: &StandardAcpLaunchExtension,
    read: impl Fn(&str) -> Option<OsString>,
) -> Result<Vec<(OsString, OsString)>, AdapterRuntimeError> {
    let mut entries = match profile.launch_policy {
        StandardAcpLaunchPolicy::OpenCodeAskConfig => {
            opencode_policy::launch_environment(profile, &read)?
        }
        StandardAcpLaunchPolicy::Inherit => {
            baseline_environment(profile, &read).ok_or(AdapterRuntimeError::LaunchUnavailable)?
        }
    };
    extend_passthrough(&mut entries, &extension.passthrough_env, &read);
    Ok(entries)
}
