use std::{fs, path::Path};

use rusqlite::{Connection, params};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    acp::intent::ImageMediaType,
    image_attachment::{ImageAttachedContext, ImageAttachmentKind},
};

use super::store::{full_path, thumb_path, validate_root};

const PNG: &[u8] = b"\x89PNG\r\n\x1a\nimage-fixture";

pub(crate) fn insert_test_image(
    connection: &Connection,
    root: &Path,
    project_id: &str,
    chat_id: &str,
    label: &str,
) -> ImageAttachedContext {
    validate_root(root).unwrap();
    let id = Uuid::new_v4().to_string();
    fs::write(full_path(root, &id), PNG).unwrap();
    fs::write(thumb_path(root, &id), PNG).unwrap();
    let hash = format!("{:x}", Sha256::digest(PNG));
    connection
        .execute(
            "INSERT INTO chat_image_artifacts(
           id,project_id,chat_id,queue_id,session_id,turn_id,lifecycle,mime_type,
           width,height,full_bytes,full_sha256,thumbnail_bytes,thumbnail_sha256,
           label,created_at,updated_at
         ) VALUES (?1,?2,?3,NULL,NULL,NULL,'pending','image/png',1,1,?4,?5,?4,?5,?6,1,1)",
            params![id, project_id, chat_id, PNG.len() as i64, hash, label],
        )
        .unwrap();
    ImageAttachedContext {
        kind: ImageAttachmentKind::Image,
        artifact_id: id,
        media_type: ImageMediaType::Png,
        width: 1,
        height: 1,
        byte_length: PNG.len() as u32,
        sha256: hash,
        label: label.into(),
    }
}
