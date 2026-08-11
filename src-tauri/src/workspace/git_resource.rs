use std::{fs, path::Path};

use crate::{
    domain::ResourceRef,
    error::{AppError, AppResult},
};

use super::{Workspace, canonical_contract_relative, join_contract_path, validate_relative_path};

impl Workspace {
    pub(crate) fn git_resource_identity(&self, resource: &ResourceRef) -> AppResult<ResourceRef> {
        if resource.root_id != self.root_id {
            return Err(AppError::InvalidRequest(
                "resource root does not match the resolved project root".into(),
            ));
        }
        validate_relative_path(&resource.relative_path, false)?;
        let candidate = self.root.join(&resource.relative_path);
        match fs::symlink_metadata(&candidate) {
            Ok(metadata) => {
                let lexical = self.lexical_git_resource(&candidate)?;
                if metadata.file_type().is_symlink() {
                    Ok(lexical)
                } else {
                    self.canonical_resource_identity(resource, false)
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.lexical_git_resource(&candidate)
            }
            Err(error) => Err(error.into()),
        }
    }

    fn lexical_git_resource(&self, candidate: &Path) -> AppResult<ResourceRef> {
        let requested_parent = candidate.parent().ok_or_else(|| {
            AppError::InvalidRequest("Git resource has no parent directory".into())
        })?;
        let mut existing_parent = requested_parent;
        loop {
            match fs::symlink_metadata(existing_parent) {
                Ok(_) => break,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    existing_parent = existing_parent.parent().ok_or_else(|| {
                        AppError::InvalidRequest(
                            "Git resource has no existing parent directory".into(),
                        )
                    })?;
                }
                Err(error) => return Err(error.into()),
            }
        }
        let canonical_parent = existing_parent.canonicalize()?;
        if !canonical_parent.starts_with(&self.root) {
            return Err(AppError::InvalidRequest(
                "Git resource parent escapes the workspace".into(),
            ));
        }
        let requested_existing_parent = canonical_contract_relative(&self.root, existing_parent)?;
        let canonical_existing_parent = canonical_contract_relative(&self.root, &canonical_parent)?;
        let same_parent = if cfg!(windows) {
            requested_existing_parent.eq_ignore_ascii_case(&canonical_existing_parent)
        } else {
            requested_existing_parent == canonical_existing_parent
        };
        if !same_parent {
            return Err(AppError::InvalidRequest(
                "Git resource parent contains a filesystem alias".into(),
            ));
        }
        let missing_suffix = canonical_contract_relative(existing_parent, candidate)?;
        let relative_path = join_contract_path(&canonical_existing_parent, &missing_suffix);
        validate_relative_path(&relative_path, false)?;
        Ok(self.resource_unchecked(&relative_path))
    }
}
