use std::sync::Arc;

use super::*;
use crate::acp::{
    attached_context::files::images::{ImagePreview, PromptImage, PromptImages},
    intent::ImageMediaType,
    limits::ECHOED_PROMPT_IMAGE_TOTAL_BYTES,
};

fn prompt(sizes: &[usize]) -> AdapterCommand {
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
    AdapterCommand::Prompt {
        prompt,
        client_message_id: RequiredText::new("client", "image-prompt").unwrap(),
        access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
    }
}

#[test]
fn pi_advertises_images_and_sends_them_as_image_content() {
    let mut runtime = fixture_runtime(41);
    ready(&mut runtime);
    assert!(runtime.capabilities().unwrap().prompt_images);
    let egress = runtime.command(prompt(&[3])).unwrap();
    let value = request(&egress);
    assert_eq!(value["type"], "prompt");
    assert_eq!(value["message"], "What is in this screenshot?");
    assert_eq!(
        value["images"],
        json!([{"type": "image", "data": "BwcH", "mimeType": "image/png"}])
    );
}

#[test]
fn a_pi_prompt_without_images_sends_no_images_field() {
    let mut runtime = fixture_runtime(42);
    ready(&mut runtime);
    let egress = runtime.command(prompt(&[])).unwrap();
    assert!(request(&egress).get("images").is_none());
}

#[test]
fn pi_images_beyond_the_echoed_bound_are_refused() {
    let half = ECHOED_PROMPT_IMAGE_TOTAL_BYTES / 2;
    let mut runtime = fixture_runtime(43);
    ready(&mut runtime);
    assert!(runtime.command(prompt(&[half, half])).is_ok());
    let mut runtime = fixture_runtime(44);
    ready(&mut runtime);
    assert!(runtime.command(prompt(&[half, half + 1])).is_err());
}
