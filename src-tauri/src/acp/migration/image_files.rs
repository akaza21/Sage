use std::{io::Read, path::Path};

use sage_core::error::{AppError, AppResult};

use super::platform;

pub(crate) fn read_image_file(path: &Path, cap: u64) -> AppResult<Vec<u8>> {
    let mut bytes = Vec::new();
    platform::open_read_no_follow(path)?
        .take(cap.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > cap {
        return Err(AppError::ResourceLimit(
            "image artifact exceeds its bound".into(),
        ));
    }
    Ok(bytes)
}

pub(crate) fn remove_image_file(path: &Path) -> AppResult<()> {
    platform::reject_path_alias(path)?;
    platform::remove_synced(path)
}
