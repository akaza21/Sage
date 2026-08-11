use std::sync::Arc;

use crate::{
    acp::{
        bindings::{adapter::AdapterRuntimeError, codex::admission::AdmittedCodexBinding},
        contract::{SageProjectId, SageRootId},
    },
    error::{AppError, AppResult},
    persistence::Database,
    project_registry::ProjectRootAuthority,
    state::ActiveProjectContext,
};

/// Codex beta is frozen to exactly one registered Project root. This wrapper
/// cannot be created by the generic characterization selected-root path.
pub(crate) struct CodexProjectRootAuthority(ProjectRootAuthority);

impl CodexProjectRootAuthority {
    #[cfg(test)]
    pub(crate) fn project_id(&self) -> &SageProjectId {
        self.0.scope().0
    }

    #[cfg(test)]
    pub(crate) fn root_id(&self) -> &SageRootId {
        self.0.scope().2
    }

    fn into_inner(self) -> ProjectRootAuthority {
        self.0
    }

    #[cfg(test)]
    pub(crate) fn for_test(authority: ProjectRootAuthority) -> Self {
        Self(authority)
    }
}

impl Database {
    pub(crate) fn codex_project_root_authority(
        &self,
        context: &ActiveProjectContext<'_>,
    ) -> AppResult<CodexProjectRootAuthority> {
        let roots = self.root_records(context.project_id().as_str())?;
        let [root] = roots.as_slice() else {
            return Err(AppError::HarnessUnavailable(
                "Codex beta requires a Project with exactly one root".into(),
            ));
        };
        let root_id = SageRootId::parse(root.id.clone()).map_err(|_| {
            AppError::InvalidRequest("Codex project root identity is invalid".into())
        })?;
        self.project_root_authority(context, &root_id)
            .map(CodexProjectRootAuthority)
    }
}

pub(super) struct RuntimeLaunchAuthority {
    binding: Option<Arc<AdmittedCodexBinding>>,
    project_root: Option<ProjectRootAuthority>,
}

impl RuntimeLaunchAuthority {
    pub(super) fn metadata_only() -> Self {
        Self {
            binding: None,
            project_root: None,
        }
    }

    pub(super) fn admitted(
        binding: Arc<AdmittedCodexBinding>,
        project_root: CodexProjectRootAuthority,
    ) -> Result<Self, AdapterRuntimeError> {
        binding
            .revalidate()
            .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        project_root
            .0
            .revalidate()
            .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        Ok(Self {
            binding: Some(binding),
            project_root: Some(project_root.into_inner()),
        })
    }

    pub(super) fn revalidate(&self) -> Result<(), AdapterRuntimeError> {
        self.binding()?
            .revalidate()
            .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        self.project_root
            .as_ref()
            .ok_or(AdapterRuntimeError::LaunchUnavailable)?
            .revalidate()
            .map_err(|_| AdapterRuntimeError::LaunchUnavailable)
    }

    pub(super) fn binding(&self) -> Result<&AdmittedCodexBinding, AdapterRuntimeError> {
        self.binding
            .as_deref()
            .ok_or(AdapterRuntimeError::LaunchUnavailable)
    }

    pub(super) fn take_project_root(
        &mut self,
    ) -> Result<ProjectRootAuthority, AdapterRuntimeError> {
        self.project_root
            .take()
            .ok_or(AdapterRuntimeError::LaunchUnavailable)
    }
}
