use serde_json::{Value, json};

use super::RequestError;
use crate::acp::intent::Prompt;
use crate::acp::limits::{LIMITS, PROMPT_FRAME_ENVELOPE_BYTES};

/// A turn's input: its text, then each attached image as a data URL. Codex
/// repeats the images in its own items, so they take the echoed bound.
pub(super) fn prompt_input(prompt: &Prompt) -> Result<Vec<Value>, RequestError> {
    let images = prompt
        .images
        .echoed()
        .map_err(|_| RequestError::FrameBytes)?;
    let text = prompt.resources.inline(prompt.as_str());
    // Codex echoes the text and every image in one inbound line; text that
    // escapes heavily could push that echo past the frame.
    if !images.is_empty() {
        let escaped = serde_json::to_string(&text).map_or(usize::MAX, |text| text.len());
        let echo =
            images.iter().map(|(_, data)| data.len()).sum::<usize>() + PROMPT_FRAME_ENVELOPE_BYTES;
        if escaped.saturating_add(echo) > LIMITS.wire.frame_bytes as usize {
            return Err(RequestError::FrameBytes);
        }
    }
    let mut input = vec![json!({"type": "text", "text": text})];
    input.extend(images.into_iter().map(|(media_type, data)| {
        json!({"type": "image", "url": format!("data:{media_type};base64,{data}")})
    }));
    Ok(input)
}
