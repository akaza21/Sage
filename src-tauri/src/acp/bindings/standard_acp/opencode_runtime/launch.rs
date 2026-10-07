use crate::acp::{
    bindings::{adapter::AdapterRuntimeError, runtime::LaunchableRuntime},
    process::{BoundedEnvironment, LaunchSpec},
};

use super::{
    super::{AcpLaunchAuthority, opencode_policy::launch_environment, profile_for},
    OpenCodeRuntime,
};

impl LaunchableRuntime for OpenCodeRuntime {
    fn launch_spec(&mut self) -> Result<LaunchSpec, AdapterRuntimeError> {
        let project_root = self
            .project_root
            .take()
            .ok_or(AdapterRuntimeError::LaunchUnavailable)?;
        let binding = self
            .registration
            .refresh()
            .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        if binding.binding_digest() != self.binding_digest {
            return Err(AdapterRuntimeError::LaunchUnavailable);
        }
        let profile = profile_for("opencode").ok_or(AdapterRuntimeError::LaunchUnavailable)?;
        let entries = launch_environment(profile, |key| std::env::var_os(key))?;
        AcpLaunchAuthority::from_admitted_binding(
            &binding,
            self.runtime_epoch.get(),
            BoundedEnvironment::new(entries).map_err(|_| AdapterRuntimeError::LaunchUnavailable)?,
        )?
        .into_launch_spec(project_root)
    }
}
