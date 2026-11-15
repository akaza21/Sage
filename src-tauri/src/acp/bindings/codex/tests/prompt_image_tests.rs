use std::sync::Arc;

use crate::acp::{
    attached_context::files::images::{ImagePreview, PromptImage, PromptImages},
    bindings::codex::request::{OutgoingRequest, RequestError},
    transcript_quote::Prompt,
};
use sage_acp_protocol::{contract::RequiredText, intent::ImageMediaType};
use sage_core::limits::ECHOED_PROMPT_IMAGE_TOTAL_BYTES;

fn prompt(sizes: &[usize]) -> Prompt {
    let mut prompt = Prompt::new("prompt", "What is in this screenshot?").unwrap();
    prompt.images = PromptImages(Arc::new(
        sizes
            .iter()
            .map(|size| PromptImage {
                media_type: ImageMediaType::Png,
                bytes: vec![7; *size],
                preview: ImagePreview::Full,
            })
            .collect(),
    ));
    prompt
}

fn turn_start(prompt: &Prompt) -> Result<OutgoingRequest, RequestError> {
    let thread = RequiredText::new("threadId", "thread-1").unwrap();
    let client = RequiredText::new("client", "client-1").unwrap();
    OutgoingRequest::turn_start(1, &thread, prompt, &client)
}

#[test]
fn a_turn_sends_its_images_after_the_text_as_data_urls() {
    let request = turn_start(&prompt(&[3])).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&request.bytes).unwrap();
    assert_eq!(
        value["params"]["input"],
        serde_json::json!([
            {"type": "text", "text": "What is in this screenshot?"},
            {"type": "image", "url": "data:image/png;base64,BwcH"}
        ])
    );
}

#[test]
fn images_at_the_echoed_bound_fit_and_beyond_it_are_refused() {
    let half = ECHOED_PROMPT_IMAGE_TOTAL_BYTES / 2;
    let request = turn_start(&prompt(&[half, half])).expect("at the bound");
    assert!(request.bytes.len() > sage_core::limits::LIMITS.wire.frame_bytes as usize / 2);
    assert!(crate::acp::transport::OutboundFrame::new_prompt(request.bytes).is_ok());
    assert!(matches!(
        turn_start(&prompt(&[half, half + 1])),
        Err(RequestError::FrameBytes)
    ));
}

#[test]
fn text_whose_escaped_echo_would_overflow_the_frame_is_refused_with_images() {
    let with = |text: &str, sizes: &[usize]| {
        let mut prompt = Prompt::new("prompt", text).unwrap();
        prompt.images = self::prompt(sizes).images;
        prompt
    };
    let images = [ECHOED_PROMPT_IMAGE_TOTAL_BYTES];
    // Each escape character echoes as six bytes.
    assert!(turn_start(&with(&"\u{1b}".repeat(30 * 1024), &images)).is_ok());
    let dense = "\u{1b}".repeat(60 * 1024);
    assert!(matches!(
        turn_start(&with(&dense, &images)),
        Err(RequestError::FrameBytes)
    ));
    assert!(turn_start(&with(&dense, &[])).is_ok());
}
