use serde::{Deserialize, Deserializer, Serialize, de};
use uuid::{Uuid, Variant, Version};

use crate::acp::intent::ImageMediaType;

pub(crate) const MAX_OUTGOING_IMAGE_BYTES: u32 = sage_core::limits::STORED_IMAGE_BYTES as u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImageAttachmentKind {
    #[serde(rename = "image")]
    Image,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageAttachedContext {
    pub kind: ImageAttachmentKind,
    pub artifact_id: String,
    pub media_type: ImageMediaType,
    pub width: u32,
    pub height: u32,
    pub byte_length: u32,
    pub sha256: String,
    pub label: String,
}

impl<'de> Deserialize<'de> for ImageAttachedContext {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Wire {
            kind: ImageAttachmentKind,
            artifact_id: String,
            media_type: ImageMediaType,
            width: u32,
            height: u32,
            byte_length: u32,
            sha256: String,
            label: String,
        }
        let value = Wire::deserialize(deserializer)?;
        let id = Uuid::parse_str(&value.artifact_id).map_err(de::Error::custom)?;
        if id.get_version() != Some(Version::Random)
            || id.get_variant() != Variant::RFC4122
            || id.hyphenated().to_string() != value.artifact_id
            || !matches!(value.media_type, ImageMediaType::Png | ImageMediaType::Jpeg)
            || value.width == 0
            || value.width > 2048
            || value.height == 0
            || value.height > 2048
            || value.byte_length == 0
            || value.byte_length > MAX_OUTGOING_IMAGE_BYTES
            || value.sha256.len() != 64
            || !value
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            || value.label.is_empty()
            || value.label.len() > 80
            || value
                .label
                .chars()
                .any(crate::acp::attached_context::has_unsafe_display_character)
        {
            return Err(de::Error::custom("invalid image attachment reference"));
        }
        Ok(Self {
            kind: value.kind,
            artifact_id: value.artifact_id,
            media_type: value.media_type,
            width: value.width,
            height: value.height,
            byte_length: value.byte_length,
            sha256: value.sha256,
            label: value.label,
        })
    }
}
