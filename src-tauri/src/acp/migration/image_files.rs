use std::{io::Read, path::Path};

use crate::error::{AppError, AppResult};

use super::platform;

pub(crate) fn publish_image_file(path: &Path, bytes: &[u8], cap: u64) -> AppResult<()> {
    platform::reject_path_alias(
        path.parent()
            .ok_or_else(|| AppError::InvalidRequest("image artifact path has no parent".into()))?,
    )?;
    let file_name = path
        .file_name()
        .ok_or_else(|| AppError::InvalidRequest("image artifact path has no file name".into()))?;
    let stage = path.with_file_name(format!(
        "{}.{}.stage",
        file_name.to_string_lossy(),
        uuid::Uuid::new_v4()
    ));
    let result = (|| {
        platform::write_bounded_create_new(&stage, bytes, cap)?;
        platform::rename_synced(&stage, path)
    })();
    if result.is_err() {
        let _ = platform::remove_synced(&stage);
    }
    result
}

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
