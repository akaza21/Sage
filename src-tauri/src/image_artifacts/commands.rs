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

use super::store::{ingest_image, remove_pending_image};

const MAX_IMAGE_BASE64_BYTES: usize = 5_592_408;

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
    let transition = state.project_transitions.lock().await;
    if request.project_generation != *transition {
        return Err("Project changed before image upload".into());
    }
    let database = state.database.clone();
    tauri::async_runtime::spawn_blocking(move || upload_image_payload(&database, request))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
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

pub(crate) fn upload_image_payload(
    database: &Database,
    request: UploadChatImageRequest,
) -> AppResult<ImageAttachedContext> {
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
    if database.active_project_id()?.as_deref() != Some(&request.project_id) {
        return Err(AppError::InvalidRequest(
            "image project is not active".into(),
        ));
    }
    let bytes = STANDARD
        .decode(&request.data_base64)
        .map_err(|_| AppError::InvalidRequest("image data is not valid base64".into()))?;
    let connection = database.write_connection()?;
    ingest_image(
        &connection,
        database.image_artifact_root()?,
        &request.project_id,
        &request.chat_id,
        request.media_type,
        &bytes,
        &request.label,
    )
}
