use std::fs::{File, OpenOptions};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;

use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT,
};

use super::super::ElevatedHelperError;
use crate::acp::bindings::codex::physical::open_exact_state_root;

const LOCK_NAME: &str = ".sage-codex-0.145.0-elevated-helper.lock";

#[derive(Debug)]
pub(super) struct TransactionLock {
    _handle: File,
    _parent_handle: File,
}

impl TransactionLock {
    pub(super) fn acquire(package_target: &Path) -> Result<Self, ElevatedHelperError> {
        let parent = package_target
            .parent()
            .ok_or(ElevatedHelperError::HandoffIdentity)?;
        let (_observed_parent, parent_handle) =
            open_exact_state_root(parent).map_err(|_| ElevatedHelperError::Drift)?;
        let handle = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .share_mode(0)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(parent.join(LOCK_NAME))
            .map_err(|_| ElevatedHelperError::Collision)?;
        let metadata = handle
            .metadata()
            .map_err(|_| ElevatedHelperError::Collision)?;
        if !metadata.file_type().is_file()
            || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
            || metadata.len() != 0
        {
            return Err(ElevatedHelperError::Collision);
        }
        handle
            .sync_all()
            .map_err(|_| ElevatedHelperError::Journal)?;
        Ok(Self {
            _handle: handle,
            _parent_handle: parent_handle,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use uuid::Uuid;

    use super::*;

    #[test]
    fn lock_is_exclusive_and_recoverable_after_owner_drop() {
        let parent =
            std::env::temp_dir().join(format!("sage-codex-helper-lock-{}", Uuid::new_v4()));
        fs::create_dir(&parent).unwrap();
        let package = parent.join("package");
        let first = TransactionLock::acquire(&package).unwrap();
        assert_eq!(
            TransactionLock::acquire(&package).unwrap_err(),
            ElevatedHelperError::Collision
        );
        drop(first);
        let recovered = TransactionLock::acquire(&package).unwrap();
        drop(recovered);
        fs::remove_file(parent.join(LOCK_NAME)).unwrap();
        fs::remove_dir(parent).unwrap();
    }
}
