use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::{
    acp::contract::{ProjectGeneration, SageChatId, SageProjectId},
    error::{AppError, AppResult},
    image_artifacts::{discard_pending_image, load_image_preview},
    state::AppState,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GetImageThumbnailRequest {
    project_id: SageProjectId,
    project_generation: ProjectGeneration,
    chat_id: SageChatId,
    artifact_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageThumbnailResponse {
    data_url: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiscardPendingImageRequest {
    project_id: SageProjectId,
    project_generation: ProjectGeneration,
    chat_id: SageChatId,
    artifact_id: String,
}

fn thumbnail_media_type(bytes: &[u8]) -> AppResult<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Ok("image/png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Ok("image/jpeg")
    } else {
        Err(AppError::InvalidRequest(
            "image thumbnail format is invalid".into(),
        ))
    }
}

#[tauri::command]
pub async fn discard_pending_chat_image(
    state: State<'_, AppState>,
    request: DiscardPendingImageRequest,
) -> Result<bool, String> {
    let _guard = state
        .lock_active_project(&request.project_id, request.project_generation)
        .await
        .map_err(|error| format!("image Project is unavailable: {error:?}"))?;
    let database = state.database.clone();
    tauri::async_runtime::spawn_blocking(move || -> AppResult<bool> {
        let root = database.image_artifact_root()?.to_path_buf();
        let mut connection = database.write_connection()?;
        discard_pending_image(
            &mut connection,
            &root,
            request.project_id.as_str(),
            request.chat_id.as_str(),
            &request.artifact_id,
        )
    })
    .await
    .map_err(|_| "image discard task failed".to_string())?
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn get_chat_image_thumbnail(
    state: State<'_, AppState>,
    request: GetImageThumbnailRequest,
) -> Result<Option<ImageThumbnailResponse>, String> {
    let _guard = state
        .lock_active_project(&request.project_id, request.project_generation)
        .await
        .map_err(|error| format!("image Project is unavailable: {error:?}"))?;
    let database = state.database.clone();
    tauri::async_runtime::spawn_blocking(move || -> AppResult<Option<ImageThumbnailResponse>> {
        let root = database.image_artifact_root()?;
        let bytes = load_image_preview(
            &database.connection(),
            root,
            request.project_id.as_str(),
            request.chat_id.as_str(),
            &request.artifact_id,
        )?;
        let Some(bytes) = bytes else { return Ok(None) };
        let media_type = thumbnail_media_type(&bytes)?;
        Ok(Some(ImageThumbnailResponse {
            data_url: format!("data:{media_type};base64,{}", STANDARD.encode(&bytes)),
        }))
    })
    .await
    .map_err(|_| "image thumbnail task failed".to_string())?
    .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::thumbnail_media_type;

    #[test]
    fn thumbnail_data_url_admits_only_png_or_jpeg_headers() {
        assert_eq!(
            thumbnail_media_type(b"\x89PNG\r\n\x1a\nbody").unwrap(),
            "image/png"
        );
        assert_eq!(
            thumbnail_media_type(&[0xff, 0xd8, 0xff, 0xe0]).unwrap(),
            "image/jpeg"
        );
        assert!(thumbnail_media_type(b"<svg>").is_err());
        assert!(thumbnail_media_type(b"\x89PNG").is_err());
    }
}
