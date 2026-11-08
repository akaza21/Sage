use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::Deserialize;
use tauri::State;

use crate::{
    acp::intent::ImageMediaType,
    error::{AppError, AppResult},
    image_attachment::ImageAttachedContext,
    persistence::Database,
    state::AppState,
};

use super::store::{ingest_image, normalize_upload, remove_pending_image};

/// Base64 of `IMAGE_UPLOAD_BYTES`; an upload is normalised before it is stored.
const MAX_IMAGE_BASE64_BYTES: usize = crate::acp::limits::IMAGE_UPLOAD_BYTES.div_ceil(3) * 4;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UploadChatImageRequest {
    pub(crate) project_id: String,
    pub(crate) project_generation: u64,
    pub(crate) chat_id: String,
    pub(crate) media_type: ImageMediaType,
    pub(crate) data_base64: String,
    pub(crate) label: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RemoveChatImageRequest {
    project_id: String,
    project_generation: u64,
    chat_id: String,
    artifact_id: String,
}

#[tauri::command]
pub(crate) async fn upload_chat_image(
    state: State<'_, AppState>,
    request: UploadChatImageRequest,
) -> Result<ImageAttachedContext, String> {
    upload_chat_image_inner(&state, request).await
}

/// The project lock is held only to check the generation and to store the
/// result. Decoding runs between, without it, under an image-work permit, so
/// a project switch never waits on a decode and decodes never overlap past
/// the image memory ceiling.
pub(crate) async fn upload_chat_image_inner(
    state: &AppState,
    request: UploadChatImageRequest,
) -> Result<ImageAttachedContext, String> {
    let generation = request.project_generation;
    let stale = || "Project changed before image upload".to_owned();
    if generation != *state.project_transitions.lock().await {
        return Err(stale());
    }
    let permit = super::work::image_work_permit().await;
    let prepared = tauri::async_runtime::spawn_blocking(move || prepare_upload(request))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())?;
    drop(permit);
    let transition = state.project_transitions.lock().await;
    if generation != *transition {
        return Err(stale());
    }
    let database = state.database.clone();
    let stored = tauri::async_runtime::spawn_blocking(move || store_upload(&database, prepared))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string());
    drop(transition);
    stored
}

#[tauri::command]
pub(crate) async fn remove_chat_image(
    state: State<'_, AppState>,
    request: RemoveChatImageRequest,
) -> Result<(), String> {
    let transition = state.project_transitions.lock().await;
    if request.project_generation != *transition {
        return Err("Project changed before image removal".into());
    }
    let database = state.database.clone();
    tauri::async_runtime::spawn_blocking(move || {
        if database.active_project_id()?.as_deref() != Some(request.project_id.as_str()) {
            return Err(AppError::InvalidRequest(
                "image project is not active".into(),
            ));
        }
        let connection = database.write_connection()?;
        remove_pending_image(
            &connection,
            database.image_artifact_root()?,
            &request.project_id,
            &request.chat_id,
            &request.artifact_id,
        )
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
}

/// An upload decoded and normalised, not yet stored.
pub(crate) struct PreparedUpload {
    project_id: String,
    chat_id: String,
    media_type: ImageMediaType,
    bytes: Vec<u8>,
    label: String,
}

/// The whole upload in one call, for tests that need no project lock.
#[cfg(test)]
pub(crate) fn upload_image_payload(
    database: &Database,
    request: UploadChatImageRequest,
) -> AppResult<ImageAttachedContext> {
    store_upload(database, prepare_upload(request)?)
}

/// Validates, decodes and normalises an upload. It touches no database.
pub(crate) fn prepare_upload(mut request: UploadChatImageRequest) -> AppResult<PreparedUpload> {
    if request.data_base64.is_empty() || request.data_base64.len() > MAX_IMAGE_BASE64_BYTES {
        return Err(AppError::ResourceLimit(
            "image upload exceeds its bound".into(),
        ));
    }
    if request.project_id.is_empty()
        || request.project_id.len() > 512
        || request.chat_id.is_empty()
        || request.chat_id.len() > 512
    {
        return Err(AppError::InvalidRequest("image owner is invalid".into()));
    }
    // Each copy is freed once the next exists: base64, then the raw file.
    let encoded = std::mem::take(&mut request.data_base64);
    let bytes = STANDARD
        .decode(&encoded)
        .map_err(|_| AppError::InvalidRequest("image data is not valid base64".into()))?;
    drop(encoded);
    let (media_type, bytes) = match normalize_upload(request.media_type, &bytes)? {
        Some(normalized) => {
            drop(bytes);
            (normalized.media_type, normalized.bytes)
        }
        None => (request.media_type, bytes),
    };
    Ok(PreparedUpload {
        project_id: request.project_id,
        chat_id: request.chat_id,
        media_type,
        bytes,
        label: request.label,
    })
}

/// Stores a prepared upload for the active project.
pub(crate) fn store_upload(
    database: &Database,
    upload: PreparedUpload,
) -> AppResult<ImageAttachedContext> {
    if database.active_project_id()?.as_deref() != Some(&upload.project_id) {
        return Err(AppError::InvalidRequest(
            "image project is not active".into(),
        ));
    }
    let connection = database.write_connection()?;
    ingest_image(
        &connection,
        database.image_artifact_root()?,
        &upload.project_id,
        &upload.chat_id,
        upload.media_type,
        &upload.bytes,
        &upload.label,
    )
}
