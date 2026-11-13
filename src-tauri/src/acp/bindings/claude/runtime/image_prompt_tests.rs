use std::sync::Arc;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};

use super::*;
use crate::acp::{
    attached_context::files::images::{ImagePreview, PromptImage, PromptImages},
    intent::ImageMediaType,
    limits::PROMPT_IMAGE_TOTAL_BYTES,
};

const PNG_1X1: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+ip1sAAAAASUVORK5CYII=";

fn runtime_offering(prompt_capabilities: Value) -> ClaudeRuntime {
    let mut runtime = ClaudeRuntime::fixture();
    initialize_with(
        &mut runtime,
        json!({
            "loadSession": true,
            "sessionCapabilities": {"resume": {}, "close": {}},
            "promptCapabilities": prompt_capabilities,
        }),
    );
    open_session(&mut runtime);
    runtime
}

fn image(bytes: Vec<u8>) -> PromptImage {
    PromptImage {
        media_type: ImageMediaType::Png,
        bytes,
        preview: ImagePreview::Full,
    }
}

fn prompt_with(images: Vec<PromptImage>) -> AdapterCommand {
    let mut prompt = Prompt::new("prompt", "What is in this screenshot?").unwrap();
    prompt.images = PromptImages(Arc::new(images));
    AdapterCommand::Prompt {
        prompt,
        client_message_id: RequiredText::new("client", "image-prompt").unwrap(),
        // Ask is the access mode that selects the session's "default" mode.
        access_mode: crate::settings_store::SessionAccessMode::Ask,
    }
}

#[test]
fn an_advertised_image_capability_sends_the_attached_png_as_an_acp_image_block() {
    let mut runtime = runtime_offering(json!({"image": true}));
    assert!(runtime.capabilities().unwrap().prompt_images);
    let png = STANDARD.decode(PNG_1X1).unwrap();

    let egress = runtime.command(prompt_with(vec![image(png)])).unwrap();

    let value: Value = serde_json::from_slice(egress.frame().payload()).unwrap();
    assert_eq!(value["method"], "session/prompt");
    assert_eq!(
        value["params"]["prompt"],
        json!([
            {"type": "text", "text": "What is in this screenshot?"},
            {"type": "image", "mimeType": "image/png", "data": PNG_1X1},
        ])
    );
}

#[test]
fn an_adapter_without_the_image_capability_refuses_the_prompt_before_any_turn() {
    for offer in [json!({}), json!({"image": false}), json!({"image": "true"})] {
        let mut runtime = runtime_offering(offer);
        assert!(!runtime.capabilities().unwrap().prompt_images);
        let png = STANDARD.decode(PNG_1X1).unwrap();

        assert!(matches!(
            runtime.command(prompt_with(vec![image(png)])),
            Err(AdapterRuntimeError::UnsupportedCommand)
        ));
        assert!(runtime.active.as_ref().unwrap().turn.is_none());
    }
}

#[test]
fn images_over_the_prompt_bound_are_refused_even_when_advertised() {
    let mut runtime = runtime_offering(json!({"image": true}));
    let half = PROMPT_IMAGE_TOTAL_BYTES / 2;
    let oversize = vec![image(vec![0x89; half]), image(vec![0x89; half + 1])];

    assert!(matches!(
        runtime.command(prompt_with(oversize)),
        Err(AdapterRuntimeError::UnsupportedCommand)
    ));
    assert!(runtime.active.as_ref().unwrap().turn.is_none());

    let five = (0..5).map(|_| image(vec![0x89; 16])).collect();
    assert!(matches!(
        runtime.command(prompt_with(five)),
        Err(AdapterRuntimeError::UnsupportedCommand)
    ));
    assert!(runtime.active.as_ref().unwrap().turn.is_none());
}

#[test]
fn a_prompt_carrying_the_full_image_budget_fits_its_frame() {
    let mut runtime = runtime_offering(json!({"image": true}));
    let quarter = PROMPT_IMAGE_TOTAL_BYTES / 4;
    let full = (0..4).map(|_| image(vec![0x89; quarter])).collect();

    let egress = runtime.command(prompt_with(full)).unwrap();

    let payload = egress.frame().payload();
    assert!(payload.len() > crate::acp::limits::LIMITS.wire.frame_bytes as usize);
    assert!(payload.len() <= crate::acp::limits::PROMPT_FRAME_BYTES);
    let value: Value = serde_json::from_slice(payload).unwrap();
    assert_eq!(value["params"]["prompt"].as_array().unwrap().len(), 5);
}
