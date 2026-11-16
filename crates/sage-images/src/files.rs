use std::{io::Read, path::Path};

use sage_core::error::{AppError, AppResult};
use sage_db::files::{open_read_no_follow, reject_path_alias, remove_synced};

pub(crate) fn read_image_file(path: &Path, cap: u64) -> AppResult<Vec<u8>> {
    let mut bytes = Vec::new();
    open_read_no_follow(path)?
        .take(cap.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > cap {
        return Err(AppError::ResourceLimit(
            "image artifact exceeds its bound".into(),
        ));
    }
    Ok(bytes)
}

pub fn remove_image_file(path: &Path) -> AppResult<()> {
    reject_path_alias(path)?;
    remove_synced(path)
}
