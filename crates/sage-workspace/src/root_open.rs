use std::path::{Path, PathBuf};

use sage_core::error::{AppError, AppResult};

use super::{Workspace, root_identity_path};

impl Workspace {
    /// Resolve a newly selected directory. Persisted roots must instead use
    /// `open_registered_root`, which refuses a changed canonical identity.
    pub fn open(project_id: &str, root_id: &str, root: &Path) -> AppResult<Self> {
        Self::from_canonical_root(project_id, root_id, root.canonicalize()?)
    }

    pub fn open_registered_root(project_id: &str, root_id: &str, root: &Path) -> AppResult<Self> {
        let canonical = root.canonicalize()?;
        if sage_core::win32_path::authority_path_key(root)
            != sage_core::win32_path::authority_path_key(&canonical)
        {
            return Err(AppError::InvalidRequest(
                "workspace root is not its stored canonical directory identity".into(),
            ));
        }
        Self::from_canonical_root(project_id, root_id, canonical)
    }

    fn from_canonical_root(project_id: &str, root_id: &str, root: PathBuf) -> AppResult<Self> {
        if !root.is_dir() {
            return Err(AppError::InvalidRequest(
                "workspace root must be a directory".into(),
            ));
        }
        Ok(Self {
            project_id: project_id.into(),
            root_id: root_id.into(),
            root_identity: root_identity_path(&root)?,
            root,
        })
    }
}
