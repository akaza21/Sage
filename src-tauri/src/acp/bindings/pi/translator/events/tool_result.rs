use serde_json::Value;

use crate::acp::bindings::pi::{
    decode::redact_structured_text,
    protocol::{AdapterError, AdapterOutput},
};

pub(super) fn tool_text(result: Option<&Value>) -> Result<Option<String>, AdapterError> {
    let Some(result) = result else {
        return Ok(None);
    };
    let content = result
        .get("content")
        .and_then(Value::as_array)
        .ok_or(AdapterError::Malformed)?;
    let mut text = String::new();
    for block in content {
        if block.get("type").and_then(Value::as_str) != Some("text") {
            continue;
        }
        let part = block
            .get("text")
            .and_then(Value::as_str)
            .ok_or(AdapterError::Malformed)?;
        if text.len().saturating_add(part.len()) > 65_536 {
            return Ok(Some("[Tool output exceeds Sage summary limit]".into()));
        }
        text.push_str(part);
    }
    Ok((!text.is_empty()).then(|| redact_structured_text(&text)))
}

pub(super) fn tool_images(result: Option<&Value>) -> Result<Vec<AdapterOutput>, AdapterError> {
    let Some(result) = result else {
        return Ok(Vec::new());
    };
    let content = result
        .get("content")
        .and_then(Value::as_array)
        .ok_or(AdapterError::Malformed)?;
    let mut images = Vec::new();
    for block in content {
        if block.get("type").and_then(Value::as_str) != Some("image") {
            continue;
        }
        if images.len() >= 4 {
            return Err(AdapterError::Limit("images per tool result"));
        }
        let media_type = block
            .get("mimeType")
            .and_then(Value::as_str)
            .ok_or(AdapterError::Malformed)?;
        let data = block
            .get("data")
            .and_then(Value::as_str)
            .ok_or(AdapterError::Malformed)?;
        if media_type.len() > 64 || data.len() > crate::acp::intent::IMAGE_BASE64_MAX {
            images.push(AdapterOutput::Unsupported {
                kind: "pi_image_oversize".into(),
            });
            continue;
        }
        images.push(AdapterOutput::Image {
            media_type: media_type.to_owned(),
            data: data.to_owned(),
        });
    }
    Ok(images)
}
