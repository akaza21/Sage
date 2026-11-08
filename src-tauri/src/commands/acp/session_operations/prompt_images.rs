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
            files::images::{ImagePreview, PromptImage, PromptImages, admit_sizes},
        },
        contract::SageAgentSessionId,
        intent::{AcpUnavailableReason, Prompt},
        limits::IMAGE_PREVIEW_BYTES,
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
        message: safe_message(
            crate::acp::attached_context::files::images::ImageRefusal::Unsupported.message(),
        ),
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
pub(super) fn attach(
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
    admit_sizes(images.iter().map(|image| image.byte_length as usize)).map_err(|refusal| {
        AcpIpcError::InvalidRequest {
            message: safe_message(refusal.message()),
        }
    })?;
    prompt.images = PromptImages(Arc::new(read(
        &state.database,
        project_id.as_str(),
        chat_id.as_str(),
        &images,
    )?));
    Ok(images
        .iter()
        .map(|image| image.artifact_id.clone())
        .collect())
}

fn read(
    database: &crate::persistence::Database,
    project_id: &str,
    chat_id: &str,
    images: &[&ImageAttachedContext],
) -> Result<Vec<PromptImage>, AcpIpcError> {
    let gone = || AcpIpcError::InvalidRequest {
        message: safe_message(
            "An attached image is no longer available. Remove it and attach it again.",
        ),
    };
    let root = database.image_artifact_root().map_err(|_| gone())?;
    let connection = database.connection();
    images
        .iter()
        .map(|image| {
            let bytes = crate::image_artifacts::verify_image_reference(
                &connection,
                root,
                project_id,
                chat_id,
                image,
            )
            .map_err(|_| gone())?;
            let preview = if bytes.len() <= IMAGE_PREVIEW_BYTES {
                ImagePreview::Full
            } else {
                match crate::image_artifacts::read_thumbnail(
                    &connection,
                    root,
                    project_id,
                    chat_id,
                    &image.artifact_id,
                ) {
                    Ok((media_type, thumbnail)) if thumbnail.len() <= IMAGE_PREVIEW_BYTES => {
                        ImagePreview::Thumbnail(media_type, thumbnail)
                    }
                    _ => ImagePreview::Unavailable,
                }
            };
            Ok(PromptImage {
                media_type: image.media_type,
                bytes,
                preview,
            })
        })
        .collect()
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
