//! Composer images on a native prompt. A session that advertises the standard
//! ACP image prompt capability receives them as image content blocks read from
//! the owner-bound image artifact store; every other send refuses them before
//! anything is prepared or dispatched.
use std::sync::Arc;

use super::*;
use crate::{
    acp::{
        attached_context::{
            AttachedContext,
            files::images::{ImagePreview, ImageRefusal, PromptImage, PromptImages, admit_sizes},
        },
        contract::SageAgentSessionId,
        intent::{AcpUnavailableReason, ImageMediaType, Prompt},
        limits::{IMAGE_PREVIEW_BYTES, PROMPT_IMAGE_COUNT},
    },
    image_attachment::ImageAttachedContext,
};

fn requested(contexts: &AttachedContexts) -> Vec<&ImageAttachedContext> {
    contexts
        .as_slice()
        .iter()
        .filter_map(|context| match context {
            AttachedContext::Image(image) => Some(image),
            _ => None,
        })
        .collect()
}

fn unsupported() -> AcpIpcError {
    AcpIpcError::Unavailable {
        reason: AcpUnavailableReason::ImageInputUnsupported,
        message: text(ImageRefusal::Unsupported.message()),
    }
}

/// Page elements must pass admission on every send target.
pub(super) fn admit_elements(contexts: &AttachedContexts) -> Result<(), AcpIpcError> {
    crate::acp::attached_context::admit_browser_elements(contexts).map_err(|refusal| {
        AcpIpcError::InvalidRequest {
            message: safe_message(refusal.message()),
        }
    })
}

/// A send that has no started session to ask, or a Steer, cannot carry images.
pub(super) fn image_input_block(contexts: &AttachedContexts) -> Result<(), AcpIpcError> {
    if !requested(contexts).is_empty() {
        return Err(unsupported());
    }
    admit_elements(contexts)
}

/// Admits the prompt's images against the session's negotiated capability and
/// the `limits.rs` bounds, then reads their verified bytes into the prompt.
/// Returns the artifact ids to retire once the prompt is handed over.
pub(super) async fn attach(
    state: &AppState,
    prompt: &mut Prompt,
    contexts: &AttachedContexts,
    project_id: &SageProjectId,
    chat_id: &SageChatId,
    session_id: &SageAgentSessionId,
) -> Result<Vec<String>, AcpIpcError> {
    let images = requested(contexts);
    if images.is_empty() {
        return Ok(Vec::new());
    }
    let page = state
        .acp
        .load_chat_page(project_id.clone(), chat_id.clone(), None)
        .map_err(store_error)?;
    if !page
        .sessions
        .as_slice()
        .iter()
        .any(|session| &session.id == session_id && session.capabilities.prompt_images)
    {
        return Err(unsupported());
    }
    if images.len() > PROMPT_IMAGE_COUNT {
        return Err(refused(ImageRefusal::TooMany));
    }
    let stored = load(
        &state.database,
        project_id.as_str(),
        chat_id.as_str(),
        &images,
    )?;
    // Decoding and re-encoding take a blocking thread, not the async runtime,
    // and an image-work permit shared with uploads (`IMAGE_WORK_PERMITS`).
    let permit = crate::image_artifacts::image_work_permit().await;
    let normalized = tauri::async_runtime::spawn_blocking(move || normalize(stored))
        .await
        .map_err(|_| internal_error())??;
    drop(permit);
    admit_sizes(normalized.iter().map(|image| image.bytes.len())).map_err(refused)?;
    prompt.images = PromptImages(Arc::new(normalized));
    Ok(images
        .iter()
        .map(|image| image.artifact_id.clone())
        .collect())
}

/// A refusal built from the `limits.rs` numbers.
fn text(message: String) -> crate::acp::contract::SafeText<8192> {
    crate::acp::contract::SafeText::new("message", message)
        .unwrap_or_else(|_| safe_message("The attached images cannot be sent."))
}

fn refused(refusal: ImageRefusal) -> AcpIpcError {
    AcpIpcError::InvalidRequest {
        message: text(refusal.message()),
    }
}

fn gone() -> AcpIpcError {
    AcpIpcError::InvalidRequest {
        message: safe_message(
            "An attached image is no longer available. Remove it and attach it again.",
        ),
    }
}

/// The verified, owner-bound stored bytes of each image.
fn load(
    database: &crate::persistence::Database,
    project_id: &str,
    chat_id: &str,
    images: &[&ImageAttachedContext],
) -> Result<Vec<(ImageMediaType, Vec<u8>)>, AcpIpcError> {
    let root = database.image_artifact_root().map_err(|_| gone())?;
    let connection = database.connection();
    images
        .iter()
        .map(|image| {
            crate::image_artifacts::verify_image_reference(
                &connection,
                root,
                project_id,
                chat_id,
                image,
            )
            .map(|bytes| (image.media_type, bytes))
            .map_err(|_| gone())
        })
        .collect()
}

/// Each image as the prompt sends it: at most `PROMPT_IMAGE_EDGE_PX` on the
/// long edge and `PROMPT_IMAGE_TOTAL_BYTES`, with a transcript preview.
fn normalize(stored: Vec<(ImageMediaType, Vec<u8>)>) -> Result<Vec<PromptImage>, AcpIpcError> {
    stored
        .into_iter()
        .map(|(media_type, bytes)| {
            let normalized = crate::image_artifacts::normalize_image(
                media_type,
                &bytes,
                crate::image_artifacts::PROMPT_IMAGE_BOUNDS,
            )
            .map_err(|error| match error {
                crate::error::AppError::ResourceLimit(_) => refused(ImageRefusal::TooLarge),
                crate::error::AppError::InvalidRequest(message) => AcpIpcError::InvalidRequest {
                    message: text(format!("An attached image cannot be sent: {message}.")),
                },
                _ => gone(),
            })?;
            drop(bytes);
            let preview = if normalized.bytes.len() <= IMAGE_PREVIEW_BYTES {
                ImagePreview::Full
            } else {
                match crate::image_artifacts::normalize_image(
                    normalized.media_type,
                    &normalized.bytes,
                    crate::image_artifacts::PREVIEW_IMAGE_BOUNDS,
                ) {
                    Ok(small) => ImagePreview::Thumbnail(small.media_type, small.bytes),
                    Err(_) => ImagePreview::Unavailable,
                }
            };
            Ok(PromptImage {
                media_type: normalized.media_type,
                bytes: normalized.bytes,
                preview,
            })
        })
        .collect()
}

#[cfg(test)]
fn read(
    database: &crate::persistence::Database,
    project_id: &str,
    chat_id: &str,
    images: &[&ImageAttachedContext],
) -> Result<Vec<PromptImage>, AcpIpcError> {
    normalize(load(database, project_id, chat_id, images)?)
}

/// Best effort: a failed retirement only delays cleanup to the next start.
pub(super) fn retire(
    state: &AppState,
    project_id: &SageProjectId,
    chat_id: &SageChatId,
    artifact_ids: &[String],
) {
    if artifact_ids.is_empty() {
        return;
    }
    let Ok(root) = state.database.image_artifact_root() else {
        return;
    };
    let ids: Vec<&str> = artifact_ids.iter().map(String::as_str).collect();
    let _ = crate::image_artifacts::retire_sent_images(
        &state.database.connection(),
        root,
        project_id.as_str(),
        chat_id.as_str(),
        &ids,
    );
}

#[cfg(test)]
#[path = "prompt_images_tests.rs"]
mod tests;
