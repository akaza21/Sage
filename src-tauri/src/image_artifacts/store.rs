use std::{fs, path::Path};

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    acp::{intent::ImageMediaType, migration},
    error::{AppError, AppResult},
    image_attachment::ImageAttachedContext,
};
const FULL_LIMIT: u64 = 4 * 1024 * 1024;
const THUMB_LIMIT: u64 = 256 * 1024;

fn invalid(message: &str) -> AppError {
    AppError::InvalidRequest(message.into())
}

pub(super) fn validate_root(root: &Path) -> AppResult<()> {
    if !root.is_absolute()
        || root.file_name().and_then(|name| name.to_str()) != Some("image-artifacts")
    {
        return Err(invalid("image artifact root is not canonical"));
    }
    fs::create_dir_all(root)?;
    let metadata = fs::symlink_metadata(root)?;
    #[cfg(unix)]
    let aliased = metadata.file_type().is_symlink();
    #[cfg(windows)]
    let aliased = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x0000_0400 != 0
    };
    if aliased || !metadata.is_dir() {
        return Err(invalid(
            "image artifact root is an alias or not a directory",
        ));
    }
    Ok(())
}

pub(super) fn full_path(root: &Path, id: &str) -> std::path::PathBuf {
    root.join(format!("{id}.full"))
}

pub(super) fn thumb_path(root: &Path, id: &str) -> std::path::PathBuf {
    root.join(format!("{id}.thumb"))
}

pub(crate) fn load_image_preview(
    connection: &Connection,
    root: &Path,
    project_id: &str,
    chat_id: &str,
    id: &str,
) -> AppResult<Option<Vec<u8>>> {
    let id = Uuid::parse_str(id).map_err(|_| invalid("image artifact id is invalid"))?;
    let id = id.to_string();
    validate_root(root)?;
    let expected: Option<(i64, String)> = connection
        .query_row(
            "SELECT thumbnail_bytes,thumbnail_sha256 FROM chat_image_artifacts
             WHERE id=?1 AND project_id=?2 AND chat_id=?3",
            params![id, project_id, chat_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((length, hash)) = expected else {
        return Ok(None);
    };
    if length <= 0 || length > THUMB_LIMIT as i64 {
        return Err(invalid("image thumbnail metadata is invalid"));
    }
    let bytes = migration::read_image_file(&thumb_path(root, &id), THUMB_LIMIT)?;
    if bytes.len() != length as usize || format!("{:x}", Sha256::digest(&bytes)) != hash {
        return Err(invalid("image thumbnail is missing or corrupt"));
    }
    Ok(Some(bytes))
}

pub(crate) fn verify_image_reference(
    connection: &Connection,
    root: &Path,
    project_id: &str,
    chat_id: &str,
    image: &ImageAttachedContext,
) -> AppResult<Vec<u8>> {
    let id =
        Uuid::parse_str(&image.artifact_id).map_err(|_| invalid("image artifact id is invalid"))?;
    if id.to_string() != image.artifact_id {
        return Err(invalid("image artifact id is not canonical"));
    }
    validate_root(root)?;
    let row: Option<(String, i64, i64, i64, String, String, String)> = connection
        .query_row(
            "SELECT mime_type,width,height,full_bytes,full_sha256,lifecycle,label
             FROM chat_image_artifacts WHERE id=?1 AND project_id=?2 AND chat_id=?3",
            params![image.artifact_id, project_id, chat_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            },
        )
        .optional()?;
    let Some((mime, width, height, length, hash, state, label)) = row else {
        return Err(invalid("image artifact is unavailable"));
    };
    let expected_mime = match image.media_type {
        ImageMediaType::Png => "image/png",
        ImageMediaType::Jpeg => "image/jpeg",
        ImageMediaType::Webp => return Err(invalid("outgoing WebP is unsupported")),
    };
    if mime != expected_mime
        || width != i64::from(image.width)
        || height != i64::from(image.height)
        || length != i64::from(image.byte_length)
        || hash != image.sha256
        || label != image.label
        || !matches!(state.as_str(), "pending" | "queued" | "inFlight")
    {
        return Err(invalid(
            "image artifact reference does not match its owner-bound record",
        ));
    }
    let bytes = migration::read_image_file(&full_path(root, &image.artifact_id), FULL_LIMIT)?;
    if bytes.len() != length as usize || format!("{:x}", Sha256::digest(&bytes)) != hash {
        return Err(invalid("image artifact is missing or corrupt"));
    }
    Ok(bytes)
}

pub(crate) fn discard_pending_image(
    connection: &mut Connection,
    root: &Path,
    project_id: &str,
    chat_id: &str,
    id: &str,
) -> AppResult<bool> {
    let parsed = Uuid::parse_str(id).map_err(|_| invalid("image artifact id is invalid"))?;
    if parsed.to_string() != id {
        return Err(invalid("image artifact id is not canonical"));
    }
    validate_root(root)?;
    let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let removed = tx.execute(
        "DELETE FROM chat_image_artifacts
         WHERE id=?1 AND project_id=?2 AND chat_id=?3 AND lifecycle='pending'",
        params![id, project_id, chat_id],
    )?;
    tx.commit()?;
    if removed == 0 {
        return Ok(false);
    }
    migration::remove_image_file(&full_path(root, id))?;
    migration::remove_image_file(&thumb_path(root, id))?;
    Ok(true)
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
