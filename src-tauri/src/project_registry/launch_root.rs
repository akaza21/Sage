use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{
    acp::contract::{ProjectGeneration, SageProjectId, SageRootId},
    error::{AppError, AppResult},
    workspace::{RootIdentity, Workspace, root_identity_path},
};

/// A retained, project-qualified authority for exactly one registered root.
/// It has no path-based production constructor and is intentionally not Clone.
pub(crate) struct ProjectRootAuthority {
    project_id: SageProjectId,
    generation: ProjectGeneration,
    root_id: SageRootId,
    canonical_path: PathBuf,
    identity: RootIdentity,
    directory: fs::File,
}

impl ProjectRootAuthority {
    pub(super) fn acquire(
        workspace: Workspace,
        project_id: SageProjectId,
        generation: ProjectGeneration,
        root_id: SageRootId,
    ) -> AppResult<Self> {
        if workspace.project_id() != project_id.as_str() || workspace.root_id() != root_id.as_str()
        {
            return Err(AppError::InvalidRequest(
                "workspace identity does not match the selected project root".into(),
            ));
        }
        let directory = open_directory(workspace.root())?;
        let identity = workspace.root_identity();
        if directory_identity(&directory)? != identity {
            return Err(AppError::InvalidRequest(
                "project root identity changed while authority was acquired".into(),
            ));
        }
        let authority = Self {
            project_id,
            generation,
            root_id,
            canonical_path: workspace.root().to_owned(),
            identity,
            directory,
        };
        authority.revalidate()?;
        Ok(authority)
    }

    pub(crate) fn scope(&self) -> (&SageProjectId, ProjectGeneration, &SageRootId) {
        (&self.project_id, self.generation, &self.root_id)
    }

    pub(crate) fn as_path(&self) -> &Path {
        &self.canonical_path
    }

    pub(crate) fn revalidate(&self) -> AppResult<()> {
        let canonical = self.canonical_path.canonicalize().map_err(|_| {
            AppError::InvalidRequest("selected project root is missing or unavailable".into())
        })?;
        if canonical != self.canonical_path || !canonical.is_dir() {
            return Err(AppError::InvalidRequest(
                "selected project root is no longer canonical".into(),
            ));
        }
        if directory_identity(&self.directory)? != self.identity
            || root_identity_path(&self.canonical_path)? != self.identity
        {
            return Err(AppError::InvalidRequest(
                "selected project root identity changed before process launch".into(),
            ));
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn for_test(
        project_id: SageProjectId,
        generation: ProjectGeneration,
        root_id: SageRootId,
        root: &Path,
    ) -> AppResult<Self> {
        let workspace = Workspace::open(project_id.as_str(), root_id.as_str(), root)?;
        Self::acquire(workspace, project_id, generation, root_id)
    }
}

#[cfg(unix)]
fn open_directory(root: &Path) -> AppResult<fs::File> {
    Ok(fs::File::open(root)?)
}

#[cfg(unix)]
fn directory_identity(directory: &fs::File) -> AppResult<RootIdentity> {
    crate::workspace::root_identity_metadata(&directory.metadata()?)
}

#[cfg(windows)]
fn open_directory(root: &Path) -> AppResult<fs::File> {
    use std::{fs::OpenOptions, os::windows::fs::OpenOptionsExt};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };

    Ok(OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        // Omitting FILE_SHARE_DELETE retains the selected directory name and
        // prevents ordinary rename/delete substitution until launch finishes.
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(root)?)
}

#[cfg(windows)]
fn directory_identity(directory: &fs::File) -> AppResult<RootIdentity> {
    crate::workspace::root_identity_file(directory)
}

#[cfg(not(any(unix, windows)))]
fn open_directory(_root: &Path) -> AppResult<fs::File> {
    Err(AppError::Unsupported(
        "safe project root authority is unsupported on this platform".into(),
    ))
}

#[cfg(not(any(unix, windows)))]
fn directory_identity(_directory: &fs::File) -> AppResult<RootIdentity> {
    Err(AppError::Unsupported(
        "safe project root authority is unsupported on this platform".into(),
    ))
}
