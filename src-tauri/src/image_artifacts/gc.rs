use std::{fs, path::Path};

use rusqlite::{Connection, OptionalExtension};
use sage_core::error::{AppError, AppResult};
use sage_db::error::SqliteResultExt;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::acp::migration;

use super::store::{full_path, thumb_path, validate_root};

fn verify_file(path: &Path, expected_length: i64, expected_hash: &str, cap: u64) -> AppResult<()> {
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
    while let Some(row) = rows.next().db()? {
        let id: String = row.get(0).db()?;
        let lifecycle: String = row.get(1).db()?;
        let full_bytes: i64 = row.get(2).db()?;
        let full_sha256: String = row.get(3).db()?;
        let thumbnail_bytes: i64 = row.get(4).db()?;
        let thumbnail_sha256: String = row.get(5).db()?;
        if lifecycle != "retired" {
            verify_file(
                &full_path(root, &id),
                full_bytes,
                &full_sha256,
                sage_core::limits::STORED_IMAGE_BYTES as u64,
            )?;
        }
        verify_file(
            &thumb_path(root, &id),
            thumbnail_bytes,
            &thumbnail_sha256,
            256 * 1024,
        )?;
    }
    drop(rows);
    drop(statement);
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
            .optional()
            .db()?;
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
