//! Image artifacts a native prompt has sent: the stored preview its transcript
//! copy reuses, and retirement once the prompt is handed to the harness.
use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};

use crate::{
    acp::{intent::ImageMediaType, migration},
    error::{AppError, AppResult},
};

use super::store::{full_path, thumb_path, validate_root};

const THUMBNAIL_LIMIT: u64 = 256 * 1024;

/// The owner-bound stored preview, checked against its recorded digest.
pub(crate) fn read_thumbnail(
    connection: &Connection,
    root: &Path,
    project_id: &str,
    chat_id: &str,
    artifact_id: &str,
) -> AppResult<(ImageMediaType, Vec<u8>)> {
    validate_root(root)?;
    let row: Option<(i64, String)> = connection
        .query_row(
            "SELECT thumbnail_bytes,thumbnail_sha256 FROM chat_image_artifacts
             WHERE id=?1 AND project_id=?2 AND chat_id=?3",
            params![artifact_id, project_id, chat_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((length, hash)) = row else {
        return Err(AppError::InvalidRequest(
            "image artifact is unavailable".into(),
        ));
    };
    let bytes = migration::read_image_file(&thumb_path(root, artifact_id), THUMBNAIL_LIMIT)?;
    if bytes.len() as i64 != length || format!("{:x}", Sha256::digest(&bytes)) != hash {
        return Err(AppError::InvalidRequest(
            "image preview is missing or corrupt".into(),
        ));
    }
    // A preview is the original bytes when small, else a PNG downscale.
    let media_type = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        ImageMediaType::Png
    } else {
        ImageMediaType::Jpeg
    };
    Ok((media_type, bytes))
}

/// Composer images become retired once their prompt was handed over: the
/// transcript keeps its own copy, reconciliation drops the row's full file,
/// and a later send can no longer reuse the artifact. Queued images stay with
/// the queue, which retires them itself.
pub(crate) fn retire_sent_images(
    connection: &Connection,
    root: &Path,
    project_id: &str,
    chat_id: &str,
    artifact_ids: &[&str],
) -> AppResult<()> {
    validate_root(root)?;
    for id in artifact_ids {
        let retired = connection.execute(
            "UPDATE chat_image_artifacts SET lifecycle='retired',updated_at=?4
             WHERE id=?1 AND project_id=?2 AND chat_id=?3 AND lifecycle='pending'",
            params![id, project_id, chat_id, crate::persistence::now_millis()],
        )?;
        if retired == 1 {
            let _ = migration::remove_image_file(&full_path(root, id));
        }
    }
    Ok(())
}
