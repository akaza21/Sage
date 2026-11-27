//! Images picked through Attach's native dialog. A picked PNG or JPEG goes
//! as image content the way an uploaded one does: read one at a time under
//! the image-work permit, normalised, then stored as a pending artifact of
//! the chat, or handed back to the new-chat draft, which keeps its images
//! until its chat exists.
use std::{fs::File, io::Read};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use sage_acp_protocol::intent::ImageMediaType;
use sage_core::limits::{IMAGE_LABEL_BYTES, IMAGE_UPLOAD_BYTES};
use sage_db::database::Database;
use sage_images::{attachment::ImageAttachedContext, remove_pending_image};
use serde::Serialize;

use crate::image_artifacts_commands::{PreparedImage, prepare_image, store_image};

/// A picked image, opened by the reader that checked it and not yet read.
pub(crate) struct PickedImage {
    file: File,
    media_type: ImageMediaType,
    label: String,
}

impl PickedImage {
    pub(super) fn new(file: File, media_type: ImageMediaType, name: &str) -> Self {
        let mut end = name.len().min(IMAGE_LABEL_BYTES);
        while !name.is_char_boundary(end) {
            end -= 1;
        }
        Self {
            file,
            media_type,
            label: name[..end].to_owned(),
        }
    }

    fn read(self) -> Result<PreparedImage, String> {
        let mut bytes = Vec::new();
        (&self.file)
            .take(IMAGE_UPLOAD_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "Cannot read attachment.")?;
        if bytes.len() > IMAGE_UPLOAD_BYTES {
            return Err(super::limits::IMAGE_OVER.clone());
        }
        drop(self.file);
        prepare_image(self.media_type, bytes, self.label).map_err(|error| error.to_string())
    }
}

/// Reads and normalises each image in turn, each under the image-work
/// permit, so at most one raw picked image is held at a time.
pub(super) async fn prepare(images: Vec<PickedImage>) -> Result<Vec<PreparedImage>, String> {
    let mut prepared = Vec::with_capacity(images.len());
    for image in images {
        let permit = sage_images::image_work_permit().await;
        let image = tauri::async_runtime::spawn_blocking(move || image.read())
            .await
            .map_err(|_| "Attachment reader failed.")??;
        drop(permit);
        prepared.push(image);
    }
    Ok(prepared)
}

/// Stores every image for the chat, or none: a failure removes the ones
/// already stored.
pub(super) fn store(
    database: &Database,
    project: &str,
    chat: &str,
    images: &[PreparedImage],
) -> Result<Vec<ImageAttachedContext>, String> {
    let mut stored = Vec::with_capacity(images.len());
    for image in images {
        match store_image(database, project, chat, image) {
            Ok(context) => stored.push(context),
            Err(error) => {
                unstore(database, project, chat, &stored);
                return Err(error.to_string());
            }
        }
    }
    Ok(stored)
}

/// Removes pending artifacts this pick stored, when the pick fails after.
pub(super) fn unstore(
    database: &Database,
    project: &str,
    chat: &str,
    images: &[ImageAttachedContext],
) {
    let Ok(connection) = database.write_connection() else {
        return;
    };
    let Ok(root) = database.image_artifact_root() else {
        return;
    };
    for image in images {
        let _ = remove_pending_image(&connection, root, project, chat, &image.artifact_id);
    }
}

/// A draft's picked image: the draft holds images as files until its
/// first send uploads them to the chat it creates.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DraftImage {
    media_type: ImageMediaType,
    label: String,
    data_base64: String,
}

pub(super) fn for_draft(images: Vec<PreparedImage>) -> Vec<DraftImage> {
    images
        .into_iter()
        .map(|image| DraftImage {
            media_type: image.media_type,
            data_base64: STANDARD.encode(&image.bytes),
            label: image.label,
        })
        .collect()
}
