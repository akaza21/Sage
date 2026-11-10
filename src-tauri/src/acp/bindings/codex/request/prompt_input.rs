use serde_json::{Value, json};

use super::RequestError;
use crate::acp::intent::Prompt;

/// A turn's input: its text, then each attached image as a data URL. Codex
/// repeats the images in its own items, so they take the echoed bound.
pub(super) fn prompt_input(prompt: &Prompt) -> Result<Vec<Value>, RequestError> {
    let images = prompt
        .images
        .echoed()
        .map_err(|_| RequestError::FrameBytes)?;
    let mut input = vec![json!({"type": "text", "text": prompt.resources.inline(prompt.as_str())})];
    input.extend(images.into_iter().map(|(media_type, data)| {
        json!({"type": "image", "url": format!("data:{media_type};base64,{data}")})
    }));
    Ok(input)
}
