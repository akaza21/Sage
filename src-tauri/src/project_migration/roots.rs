use std::path::{Component, Path, PathBuf};

use crate::{
    domain::ResourceRef,
    error::{AppError, AppResult},
    project_registry::persisted_path_text,
    workspace::{Workspace, validate_contract_path},
};

pub(super) fn canonical_migrated_path(
    project_id: &str,
    root_id: &str,
    root: &Path,
    relative_path: &str,
) -> AppResult<String> {
    validate_contract_path(relative_path, false)?;
    let Ok(workspace) = Workspace::open(project_id, root_id, root) else {
        return Ok(relative_path.into());
    };
    let resource = ResourceRef {
        root_id: root_id.into(),
        relative_path: relative_path.into(),
    };
    if workspace.root().join(relative_path).try_exists()? {
        return workspace
            .canonical_resource_identity(&resource, false)
            .map(|canonical| canonical.relative_path);
    }
    Ok(relative_path.into())
}

pub(super) fn canonical_or_recorded_root(recorded: &str) -> AppResult<PathBuf> {
    let path = Path::new(recorded);
    if let Ok(canonical) = path.canonicalize() {
        if !canonical.is_dir() {
            return Err(AppError::InvalidRequest(
                "legacy project root is not a directory".into(),
            ));
        }
        persisted_path_text(&canonical)?;
        return Ok(canonical);
    }
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
    {
        return Err(AppError::InvalidRequest(
            "unavailable legacy project root has no safe canonical identity".into(),
        ));
    }
    let normalized = path.components().collect::<PathBuf>();
    if normalized.to_str() != Some(recorded) {
        return Err(AppError::InvalidRequest(
            "unavailable legacy project root has a noncanonical recorded spelling".into(),
        ));
    }
    Ok(path.to_path_buf())
}
