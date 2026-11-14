use std::sync::Arc;

use sage_core::error::{AppError, AppResult};

use crate::project_registry::ProjectRegistryStore;
use crate::{
    acp::bindings::{adapter::AdapterRuntimeError, codex::admission::AdmittedCodexBinding},
    project_registry::ProjectRootAuthority,
    state::ActiveProjectContext,
};
use sage_acp_protocol::contract::{SageProjectId, SageRootId};
use sage_db::database::Database;

/// Codex beta is frozen to exactly one registered Project root. This wrapper
/// cannot be created by the generic characterization selected-root path.
pub(crate) struct CodexProjectRootAuthority(ProjectRootAuthority);

impl CodexProjectRootAuthority {
    pub(crate) fn project_id(&self) -> &SageProjectId {
        self.0.scope().0
    }

    pub(crate) fn project_generation(&self) -> sage_acp_protocol::contract::ProjectGeneration {
        self.0.scope().1
    }

    pub(crate) fn root_id(&self) -> &SageRootId {
        self.0.scope().2
    }

    pub(crate) fn identity_bytes(&self) -> [u8; 24] {
        self.0.identity_bytes()
    }

    pub(crate) fn as_path(&self) -> &std::path::Path {
        self.0.as_path()
    }

    pub(crate) fn revalidate(&self) -> AppResult<()> {
        self.0.revalidate()
    }

    fn into_inner(self) -> ProjectRootAuthority {
        self.0
    }

    #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
    pub(crate) fn into_project_root(self) -> ProjectRootAuthority {
        self.into_inner()
    }

    #[cfg(test)]
    pub(crate) fn for_test(authority: ProjectRootAuthority) -> Self {
        Self(authority)
    }

    #[cfg(test)]
    pub(crate) fn clone_for_test(&self) -> Self {
        Self(
            ProjectRootAuthority::for_test(
                self.project_id().clone(),
                self.project_generation(),
                self.root_id().clone(),
                self.as_path(),
            )
            .expect("retained test root authority is valid"),
        )
    }

    #[cfg(test)]
    pub(crate) fn adopt_foreign_root_identity_for_test(
        &mut self,
        other_root: &std::path::Path,
    ) -> AppResult<()> {
        self.0.adopt_foreign_root_identity_for_test(other_root)
    }
}

/// Codex's project root authority, on the shared database.
pub(crate) trait CodexRootAuthorityStore {
    fn codex_project_root_authority(
        &self,
        context: &ActiveProjectContext<'_>,
    ) -> AppResult<CodexProjectRootAuthority>;
}

impl CodexRootAuthorityStore for Database {
    fn codex_project_root_authority(
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

    #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
    pub(super) fn project_root_path(&self) -> Result<&std::path::Path, AdapterRuntimeError> {
        self.project_root
            .as_ref()
            .map(ProjectRootAuthority::as_path)
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
