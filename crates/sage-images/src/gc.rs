use std::{fs, path::Path};

use rusqlite::{Connection, OptionalExtension};
use sage_core::error::{AppError, AppResult};
use sage_db::error::SqliteResultExt;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::files::{read_image_file, remove_image_file};

use super::store::{full_path, thumb_path, validate_root};

fn verify_file(path: &Path, expected_length: i64, expected_hash: &str, cap: u64) -> AppResult<()> {
    if expected_length <= 0 || expected_length as u64 > cap {
        return Err(AppError::InvalidRequest(
            "image artifact metadata is invalid".into(),
        ));
    }
    let bytes = read_image_file(path, cap)?;
    if bytes.len() != expected_length as usize
        || format!("{:x}", Sha256::digest(&bytes)) != expected_hash
    {
        return Err(AppError::InvalidRequest(
            "image artifact file is missing or corrupt".into(),
        ));
    }
    Ok(())
}

fn artifact_id(file_name: &str) -> Option<(&str, bool)> {
    let (id, full) = if let Some(id) = file_name.strip_suffix(".full") {
        (id, true)
    } else {
        (file_name.strip_suffix(".thumb")?, false)
    };
    let uuid = Uuid::parse_str(id).ok()?;
    (uuid.to_string() == id).then_some((id, full))
}

fn owned_stage(file_name: &str) -> bool {
    let Some(without_stage) = file_name.strip_suffix(".stage") else {
        return false;
    };
    let Some((artifact, nonce)) = without_stage.rsplit_once('.') else {
        return false;
    };
    artifact_id(artifact).is_some()
        && Uuid::parse_str(nonce).is_ok_and(|id| id.to_string() == nonce)
}

/// Drops pending rows, sweeps owned orphans and stage files, and returns how
/// many live rows point at a missing or corrupt file. Those rows stay: every
/// read verifies its bytes and fails closed, so a damaged store (a copied
/// profile without `image-artifacts`, say) degrades instead of blocking startup.
pub fn reconcile_image_artifacts(connection: &Connection, root: &Path) -> AppResult<usize> {
    validate_root(root)?;
    connection
        .execute(
            "DELETE FROM chat_image_artifacts WHERE lifecycle='pending'",
            [],
        )
        .db()?;
    let mut statement = connection
        .prepare(
            "SELECT id,lifecycle,full_bytes,full_sha256,thumbnail_bytes,thumbnail_sha256
         FROM chat_image_artifacts ORDER BY id",
        )
        .db()?;
    let mut rows = statement.query([]).db()?;
    let mut damaged = 0;
    while let Some(row) = rows.next().db()? {
        let id: String = row.get(0).db()?;
        let lifecycle: String = row.get(1).db()?;
        let full_bytes: i64 = row.get(2).db()?;
        let full_sha256: String = row.get(3).db()?;
        let thumbnail_bytes: i64 = row.get(4).db()?;
        let thumbnail_sha256: String = row.get(5).db()?;
        let full_ok = lifecycle == "retired"
            || verify_file(
                &full_path(root, &id),
                full_bytes,
                &full_sha256,
                sage_core::limits::STORED_IMAGE_BYTES as u64,
            )
            .is_ok();
        let thumb_ok = verify_file(
            &thumb_path(root, &id),
            thumbnail_bytes,
            &thumbnail_sha256,
            256 * 1024,
        )
        .is_ok();
        if !(full_ok && thumb_ok) {
            damaged += 1;
        }
    }
    drop(rows);
    drop(statement);
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if owned_stage(name) {
            remove_image_file(&entry.path())?;
            continue;
        }
        let Some((id, full)) = artifact_id(name) else {
            continue;
        };
        let lifecycle: Option<String> = connection
            .query_row(
                "SELECT lifecycle FROM chat_image_artifacts WHERE id=?1",
                [id],
                |row| row.get(0),
            )
            .optional()
            .db()?;
        if lifecycle.is_none() || (full && lifecycle.as_deref() == Some("retired")) {
            remove_image_file(&entry.path())?;
        }
    }
    Ok(damaged)
}
