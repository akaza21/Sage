use serde_json::{Value, json};

use super::RequestError;
use crate::acp::intent::Prompt;

/// Codex repeats the images in its own items, so they take the echoed bound.
pub(super) fn prompt_input(prompt: &Prompt) -> Result<Vec<Value>, RequestError> {
    let images = prompt
        .images
        .echoed()
        .map_err(|_| RequestError::FrameBytes)?;
    let text = prompt.resources.inline(prompt.as_str());
    // Codex echoes the text and every image in one inbound line.
    if !prompt.images.echo_fits(&text) {
        return Err(RequestError::FrameBytes);
    }
    let mut input = vec![json!({"type": "text", "text": text})];
    input.extend(images.into_iter().map(|(media_type, data)| {
        json!({"type": "image", "url": format!("data:{media_type};base64,{data}")})
    }));
    Ok(input)
}
