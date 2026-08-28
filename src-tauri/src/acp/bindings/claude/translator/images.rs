use serde_json::{Map, Value};

use crate::acp::{
    image_admit::{ImageAdmitError, admit_image_parts},
    intent::ImageMediaType,
    lifecycle::ItemLifecycle,
    limits::LIMITS,
};

use super::{
    AdapterEvent, ClaudeTranslator, TranslationError, optional_external_key, unsupported_event,
};

impl ClaudeTranslator {
    pub(super) fn image_event(
        &mut self,
        update: &Map<String, Value>,
        content: &Map<String, Value>,
    ) -> Result<AdapterEvent, TranslationError> {
        if content
            .get("uri")
            .and_then(Value::as_str)
            .is_some_and(|uri| !uri.is_empty())
        {
            return rejected_image(ImageAdmitError::UnsupportedMediaType);
        }
        let media_type = content
            .get("mimeType")
            .and_then(Value::as_str)
            .and_then(ImageMediaType::parse);
        let Some(media_type) = media_type else {
            return rejected_image(ImageAdmitError::UnsupportedMediaType);
        };
        let encoded = content.get("data").and_then(Value::as_str).unwrap_or("");
        let payload = match admit_image_parts(media_type, encoded) {
            Ok(payload) => payload,
            Err(reason) => return rejected_image(reason),
        };
        let external_key = optional_external_key(update, "agent-image")?;
        let key = external_key.as_str().to_owned();
        if !self.image_keys.contains(&key)
            && self.tracked_item_count() >= LIMITS.wire.items_per_turn as usize
        {
            return Err(TranslationError::ItemLimit);
        }
        self.image_keys.insert(key);
        Ok(AdapterEvent::Item {
            external_key,
            lifecycle: ItemLifecycle::Completed,
            tool_state: None,
            payload,
        })
    }
}

fn rejected_image(reason: ImageAdmitError) -> Result<AdapterEvent, TranslationError> {
    unsupported_event("agent_message_content", reason.unsupported_summary())
}
