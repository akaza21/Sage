use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use super::{HANDOFF_NAME, PACKAGE_NAME, ProvisioningError, STATE_NAME};

pub(super) fn validate_root_closure(root: &Path) -> Result<(), ProvisioningError> {
    let names = fs::read_dir(root)
        .map_err(|_| ProvisioningError::StageDrift)?
        .map(|entry| {
            entry
                .map(|entry| entry.file_name())
                .map_err(|_| ProvisioningError::StageDrift)
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    let expected = [HANDOFF_NAME, PACKAGE_NAME, STATE_NAME]
        .into_iter()
        .map(Into::into)
        .collect::<BTreeSet<_>>();
    if names != expected {
        return Err(ProvisioningError::StageDrift);
    }
    Ok(())
}

pub(super) fn require_absent(path: &Path) -> Result<(), ProvisioningError> {
    if target_exists(path)? {
        Err(ProvisioningError::TargetExists)
    } else {
        Ok(())
    }
}

pub(super) fn require_present(path: &Path) -> Result<(), ProvisioningError> {
    if target_exists(path)? {
        Ok(())
    } else {
        Err(ProvisioningError::StageDrift)
    }
}

pub(super) fn target_exists(path: &Path) -> Result<bool, ProvisioningError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(ProvisioningError::Io),
    }
}
