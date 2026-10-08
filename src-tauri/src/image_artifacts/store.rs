use std::{fs, path::Path};

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    acp::{intent::ImageMediaType, migration},
    error::{AppError, AppResult},
    image_attachment::{ImageAttachedContext, ImageAttachmentKind},
    persistence::now_millis,
};

use super::normalize::{make_thumbnail, normalize_image};

const CHAT_LIMIT: i64 = 32 * 1024 * 1024;
const PROJECT_LIMIT: i64 = 128 * 1024 * 1024;
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

fn used_bytes(connection: &Connection, column: &str, owner: &str) -> AppResult<i64> {
    let sql = match column {
        "chat_id" => {
            "SELECT COALESCE(SUM(CASE WHEN lifecycle='retired' THEN thumbnail_bytes ELSE full_bytes+thumbnail_bytes END),0) FROM chat_image_artifacts WHERE chat_id=?1"
        }
        "project_id" => {
            "SELECT COALESCE(SUM(CASE WHEN lifecycle='retired' THEN thumbnail_bytes ELSE full_bytes+thumbnail_bytes END),0) FROM chat_image_artifacts WHERE project_id=?1"
        }
        _ => return Err(invalid("invalid artifact quota scope")),
    };
    Ok(connection.query_row(sql, [owner], |row| row.get(0))?)
}

fn active_bytes(connection: &Connection, column: &str, owner: &str) -> AppResult<i64> {
    let sql = match column {
        "chat_id" => {
            "SELECT COALESCE(SUM(full_bytes+thumbnail_bytes),0) FROM chat_image_artifacts WHERE chat_id=?1 AND lifecycle!='retired'"
        }
        "project_id" => {
            "SELECT COALESCE(SUM(full_bytes+thumbnail_bytes),0) FROM chat_image_artifacts WHERE project_id=?1 AND lifecycle!='retired'"
        }
        _ => return Err(invalid("invalid artifact quota scope")),
    };
    Ok(connection.query_row(sql, [owner], |row| row.get(0))?)
}

fn admit_quota(
    transaction: &rusqlite::Transaction<'_>,
    project_id: &str,
    chat_id: &str,
    added: i64,
    chat_limit: i64,
    project_limit: i64,
) -> AppResult<Vec<String>> {
    if active_bytes(transaction, "chat_id", chat_id)? + added > chat_limit
        || active_bytes(transaction, "project_id", project_id)? + added > project_limit
    {
        return Err(AppError::ResourceLimit(
            "active image artifact quota is full".into(),
        ));
    }
    let mut evicted = Vec::new();
    loop {
        let chat_over = used_bytes(transaction, "chat_id", chat_id)? + added > chat_limit;
        let project_over =
            used_bytes(transaction, "project_id", project_id)? + added > project_limit;
        if !chat_over && !project_over {
            break;
        }
        let oldest: Option<String> = if chat_over {
            transaction.query_row(
                "SELECT id FROM chat_image_artifacts WHERE project_id=?1 AND chat_id=?2 AND lifecycle='retired' ORDER BY created_at,id LIMIT 1",
                params![project_id, chat_id], |row| row.get(0),
            ).optional()?
        } else {
            transaction.query_row(
                "SELECT id FROM chat_image_artifacts WHERE project_id=?1 AND lifecycle='retired' ORDER BY created_at,id LIMIT 1",
                [project_id], |row| row.get(0),
            ).optional()?
        };
        let id =
            oldest.ok_or_else(|| AppError::ResourceLimit("image artifact quota is full".into()))?;
        transaction.execute("DELETE FROM chat_image_artifacts WHERE id=?1", [&id])?;
        evicted.push(id);
    }
    Ok(evicted)
}

pub(crate) fn create_pending_image(
    connection: &mut Connection,
    root: &Path,
    project_id: &str,
    chat_id: &str,
    encoded: &[u8],
    label: &str,
) -> AppResult<ImageAttachedContext> {
    if label.is_empty()
        || label.len() > 80
        || label
            .chars()
            .any(crate::acp::attached_context::has_unsafe_display_character)
    {
        return Err(invalid("image label is invalid"));
    }
    validate_root(root)?;
    let image = normalize_image(encoded).map_err(invalid)?;
    let thumb = make_thumbnail(&image).map_err(invalid)?;
    let id = Uuid::new_v4().to_string();
    let full = full_path(root, &id);
    let preview = thumb_path(root, &id);
    let added = (image.bytes.len() + thumb.bytes.len()) as i64;
    let now = now_millis();
    let result = (|| -> AppResult<Vec<String>> {
        migration::publish_image_file(&full, &image.bytes, FULL_LIMIT)?;
        migration::publish_image_file(&preview, &thumb.bytes, THUMB_LIMIT)?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let owner_exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM chats WHERE project_id=?1 AND id=?2)",
            params![project_id, chat_id],
            |row| row.get(0),
        )?;
        if !owner_exists {
            return Err(invalid("image attachment owner is unavailable"));
        }
        let evicted = admit_quota(&tx, project_id, chat_id, added, CHAT_LIMIT, PROJECT_LIMIT)?;
        tx.execute(
            "INSERT INTO chat_image_artifacts(
               id,project_id,chat_id,queue_id,session_id,turn_id,lifecycle,mime_type,
               width,height,full_bytes,full_sha256,thumbnail_bytes,thumbnail_sha256,
               label,created_at,updated_at
             ) VALUES (?1,?2,?3,NULL,NULL,NULL,'pending',?4,?5,?6,?7,?8,?9,?10,?11,?12,?12)",
            params![
                id,
                project_id,
                chat_id,
                image.media_type,
                image.width,
                image.height,
                image.bytes.len() as i64,
                image.sha256,
                thumb.bytes.len() as i64,
                thumb.sha256,
                label,
                now
            ],
        )?;
        tx.commit()?;
        Ok(evicted)
    })();
    if result.is_err() {
        let _ = migration::remove_image_file(&preview);
        let _ = migration::remove_image_file(&full);
    }
    for retired in result? {
        let _ = migration::remove_image_file(&thumb_path(root, &retired));
        let _ = migration::remove_image_file(&full_path(root, &retired));
    }
    Ok(ImageAttachedContext {
        kind: ImageAttachmentKind::Image,
        artifact_id: id,
        media_type: if image.media_type == "image/png" {
            ImageMediaType::Png
        } else {
            ImageMediaType::Jpeg
        },
        width: image.width,
        height: image.height,
        byte_length: image.bytes.len() as u32,
        sha256: image.sha256,
        label: label.into(),
    })
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
