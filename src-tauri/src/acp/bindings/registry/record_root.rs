use super::RegistryAdmissionUnavailable;
use crate::acp::{
    bindings::codex::CodexProjectRootAuthority, resource_mapper::ConfiguredResourceRoot,
};
use crate::project_registry::ProjectRegistryStore;

#[derive(Debug, Clone, Copy)]
pub(super) enum RootPolicy {
    Codex,
    Claude,
    StandardAcp,
    Pi,
}

pub(crate) enum BindingProjectRoot {
    Codex(CodexProjectRootAuthority),
    Claude(crate::project_registry::ProjectRootAuthority),
    StandardAcp(crate::project_registry::ProjectRootAuthority),
    Pi(crate::project_registry::ProjectRootAuthority),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum BindingProjectRootError {
    #[error("the admitted project root no longer revalidates")]
    Revalidation,
    #[error("the admitted project root is not valid UTF-8")]
    NonUnicode,
    #[error("the admitted project root is empty")]
    Empty,
}

pub(super) fn single_root_authority(
    database: &sage_db::database::Database,
    context: &crate::state::ActiveProjectContext<'_>,
) -> Result<crate::project_registry::ProjectRootAuthority, ()> {
    let roots = database
        .root_records(context.project_id().as_str())
        .map_err(|_| ())?;
    let [root] = roots.as_slice() else {
        return Err(());
    };
    let root_id =
        sage_acp_protocol::contract::SageRootId::parse(root.id.clone()).map_err(|_| ())?;
    database
        .project_root_authority(context, &root_id)
        .map_err(|_| ())
}

impl BindingProjectRoot {
    pub(crate) fn project_matches(
        &self,
        project_id: &sage_acp_protocol::contract::SageProjectId,
    ) -> bool {
        match self {
            Self::Codex(root) => root.project_id() == project_id,
            Self::Claude(root) => root.scope().0 == project_id,
            Self::StandardAcp(root) => root.scope().0 == project_id,
            Self::Pi(root) => root.scope().0 == project_id,
        }
    }

    pub(crate) fn revalidate(&self) -> Result<(), BindingProjectRootError> {
        match self {
            Self::Codex(root) => root
                .revalidate()
                .map_err(|_| BindingProjectRootError::Revalidation),
            Self::Claude(root) => root
                .revalidate()
                .map_err(|_| BindingProjectRootError::Revalidation),
            Self::StandardAcp(root) => root
                .revalidate()
                .map_err(|_| BindingProjectRootError::Revalidation),
            Self::Pi(root) => root
                .revalidate()
                .map_err(|_| BindingProjectRootError::Revalidation),
        }
    }

    pub(crate) fn native_root(&self) -> Result<String, BindingProjectRootError> {
        let path = match self {
            Self::Codex(root) => root.as_path(),
            Self::Claude(root) => root.as_path(),
            Self::StandardAcp(root) => root.as_path(),
            Self::Pi(root) => root.as_path(),
        };
        let path = path.to_str().ok_or(BindingProjectRootError::NonUnicode)?;
        let path = path
            .strip_prefix(r"\\?\")
            .or_else(|| path.strip_prefix(r"//?/"))
            .unwrap_or(path);
        (!path.is_empty())
            .then(|| path.to_owned())
            .ok_or(BindingProjectRootError::Empty)
    }

    pub(crate) fn codex_project_root(
        &self,
    ) -> Result<&CodexProjectRootAuthority, RegistryAdmissionUnavailable> {
        match self {
            Self::Codex(root) => Ok(root),
            Self::Claude(_) | Self::StandardAcp(_) => Err(RegistryAdmissionUnavailable::Drift),
            Self::Pi(_) => Err(RegistryAdmissionUnavailable::Drift),
        }
    }

    pub(crate) fn configured_resource_root(&self) -> ConfiguredResourceRoot {
        match self {
            Self::Codex(root) => ConfiguredResourceRoot {
                root_id: root.root_id().clone(),
                canonical_native_root: root.as_path().to_owned(),
            },
            Self::Claude(root) => ConfiguredResourceRoot {
                root_id: root.scope().2.clone(),
                canonical_native_root: root.as_path().to_owned(),
            },
            Self::StandardAcp(root) => ConfiguredResourceRoot {
                root_id: root.scope().2.clone(),
                canonical_native_root: root.as_path().to_owned(),
            },
            Self::Pi(root) => ConfiguredResourceRoot {
                root_id: root.scope().2.clone(),
                canonical_native_root: root.as_path().to_owned(),
            },
        }
    }
}
