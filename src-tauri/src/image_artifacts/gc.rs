use std::{fs, path::Path};

use rusqlite::{Connection, OptionalExtension};
use sage_core::error::{AppError, AppResult};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::acp::migration;

use sage_acp_protocol::intent::ImageMediaType;
use sage_core::limits::STORED_THUMBNAIL_EDGE_PX;

use super::{
    normalize::dimensions,
    store::{THUMBNAIL_LIMIT, full_path, thumb_path, validate_root},
    thumbnail::square_thumbnail,
};

/// The largest `.thumb` an earlier build wrote: a small image kept whole.
const LEGACY_THUMBNAIL_LIMIT: u64 = 256 * 1024;

fn verify_file(
    path: &Path,
    expected_length: i64,
    expected_hash: &str,
    cap: u64,
) -> AppResult<Vec<u8>> {
    if expected_length <= 0 || expected_length as u64 > cap {
        return Err(AppError::InvalidRequest(
            "image artifact metadata is invalid".into(),
        ));
    }
    let bytes = migration::read_image_file(path, cap)?;
    if bytes.len() != expected_length as usize
        || format!("{:x}", Sha256::digest(&bytes)) != expected_hash
    {
        return Err(AppError::InvalidRequest(
            "image artifact file is missing or corrupt".into(),
        ));
    }
    Ok(bytes)
}

fn is_chip_thumbnail(bytes: &[u8]) -> bool {
    dimensions(ImageMediaType::Png, bytes)
        .is_ok_and(|(width, height)| width.max(height) <= STORED_THUMBNAIL_EDGE_PX)
}

/// Rewrites the thumbnail of a live image from its verified full image: one
/// an earlier build wrote (a small image kept whole, or a squashed 128 px
/// copy), or one a crash left out of step with its row. The file is replaced
/// before the row, so an interrupted rebuild is redone at the next start.
fn rebuild_thumbnail(
    connection: &Connection,
    root: &Path,
    id: &str,
    mime_type: &str,
) -> AppResult<()> {
    let media_type = match mime_type {
        "image/png" => ImageMediaType::Png,
        "image/jpeg" => ImageMediaType::Jpeg,
        _ => {
            return Err(AppError::InvalidRequest(
                "image artifact type is unsupported".into(),
            ));
        }
    };
    let full = migration::read_image_file(
        &full_path(root, id),
        sage_core::limits::STORED_IMAGE_BYTES as u64,
    )?;
    let thumbnail = square_thumbnail(media_type, &full, STORED_THUMBNAIL_EDGE_PX)?;
    drop(full);
    let stage = root.join(format!("{id}.thumb.{}.stage", Uuid::new_v4()));
    let written = (|| -> AppResult<()> {
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&stage)?;
        file.write_all(&thumbnail)?;
        file.sync_all()?;
        fs::rename(&stage, thumb_path(root, id))?;
        Ok(())
    })();
    if written.is_err() {
        let _ = fs::remove_file(&stage);
    }
    written?;
    connection.execute(
        "UPDATE chat_image_artifacts SET thumbnail_bytes=?1,thumbnail_sha256=?2 WHERE id=?3",
        rusqlite::params![
            thumbnail.len() as i64,
            format!("{:x}", Sha256::digest(&thumbnail)),
            id
        ],
    )?;
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

pub(crate) fn reconcile_image_artifacts(connection: &Connection, root: &Path) -> AppResult<()> {
    validate_root(root)?;
    connection.execute(
        "DELETE FROM chat_image_artifacts WHERE lifecycle='pending'",
        [],
    )?;
    let mut statement = connection.prepare(
        "SELECT id,lifecycle,mime_type,full_bytes,full_sha256,thumbnail_bytes,thumbnail_sha256
         FROM chat_image_artifacts ORDER BY id",
    )?;
    let mut rows = statement.query([])?;
    let mut stale_thumbnails = Vec::new();
    while let Some(row) = rows.next()? {
        let id: String = row.get(0)?;
        let lifecycle: String = row.get(1)?;
        let mime_type: String = row.get(2)?;
        let full_bytes: i64 = row.get(3)?;
        let full_sha256: String = row.get(4)?;
        let thumbnail_bytes: i64 = row.get(5)?;
        let thumbnail_sha256: String = row.get(6)?;
        let thumbnail = thumb_path(root, &id);
        if lifecycle == "retired" {
            verify_file(
                &thumbnail,
                thumbnail_bytes,
                &thumbnail_sha256,
                LEGACY_THUMBNAIL_LIMIT,
            )?;
            continue;
        }
        verify_file(
            &full_path(root, &id),
            full_bytes,
            &full_sha256,
            sage_core::limits::STORED_IMAGE_BYTES as u64,
        )?;
        let current = verify_file(
            &thumbnail,
            thumbnail_bytes,
            &thumbnail_sha256,
            THUMBNAIL_LIMIT,
        )
        .ok()
        .is_some_and(|bytes| is_chip_thumbnail(&bytes));
        if !current {
            stale_thumbnails.push((id, mime_type));
        }
    }
    drop(rows);
    drop(statement);
    for (id, mime_type) in stale_thumbnails {
        rebuild_thumbnail(connection, root, &id, &mime_type)?;
    }
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if owned_stage(name) {
            migration::remove_image_file(&entry.path())?;
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
            .optional()?;
        if lifecycle.is_none() || (full && lifecycle.as_deref() == Some("retired")) {
            migration::remove_image_file(&entry.path())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconciliation_removes_owned_orphans_and_rejects_live_corruption() {
        let opened = migration::open_or_migrate(Path::new(":memory:")).unwrap();
        let (connection, _) = opened.into_parts();
        connection.execute("INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('p','P',1,1,1)", []).unwrap();
        connection.execute("INSERT INTO chats(id,project_id,ordinal,title,created_at,updated_at) VALUES('c','p',0,'C',1,1)", []).unwrap();
        let root = std::env::temp_dir()
            .join(format!("sage-image-gc-{}", Uuid::new_v4()))
            .join("image-artifacts");
        let image =
            crate::image_artifacts::insert_test_image(&connection, &root, "p", "c", "image.png");
        connection
            .execute(
                "UPDATE chat_image_artifacts SET lifecycle='queued',queue_id='q1' WHERE id=?1",
                [&image.artifact_id],
            )
            .unwrap();
        let orphan = full_path(&root, &Uuid::new_v4().to_string());
        fs::write(&orphan, b"orphan").unwrap();
        reconcile_image_artifacts(&connection, &root).unwrap();
        assert!(!orphan.exists());
        fs::write(full_path(&root, &image.artifact_id), b"tampered").unwrap();
        assert!(reconcile_image_artifacts(&connection, &root).is_err());
        fs::remove_file(full_path(&root, &image.artifact_id)).unwrap();
        fs::remove_file(thumb_path(&root, &image.artifact_id)).unwrap();
        fs::remove_dir(&root).unwrap();
        fs::remove_dir(root.parent().unwrap()).unwrap();
    }
}
