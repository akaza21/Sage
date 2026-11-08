//! Images a native prompt carries as standard ACP image content blocks. The
//! bytes come from the owner-bound image artifact store and live only for one
//! dispatch; nothing here is persisted except each transcript preview.
use std::sync::Arc;

use base64::{Engine as _, engine::general_purpose::STANDARD};

use crate::acp::{
    intent::{CapabilitySnapshotV1, ImageMediaType},
    limits::{PROMPT_IMAGE_COUNT, PROMPT_IMAGE_TOTAL_BYTES},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PromptImage {
    pub media_type: ImageMediaType,
    pub bytes: Vec<u8>,
    /// The transcript copy: `Full` when `bytes` fit one normalized item,
    /// otherwise a smaller re-encoding, or nothing when neither fits.
    pub preview: ImagePreview,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ImagePreview {
    Full,
    Thumbnail(ImageMediaType, Vec<u8>),
    Unavailable,
}

impl PromptImage {
    pub(crate) fn preview(&self) -> Option<(ImageMediaType, &[u8])> {
        match &self.preview {
            ImagePreview::Full => Some((self.media_type, &self.bytes)),
            ImagePreview::Thumbnail(media_type, bytes) => Some((*media_type, bytes)),
            ImagePreview::Unavailable => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PromptImages(pub Arc<Vec<PromptImage>>);

impl Default for PromptImages {
    fn default() -> Self {
        static EMPTY: std::sync::OnceLock<Arc<Vec<PromptImage>>> = std::sync::OnceLock::new();
        Self(Arc::clone(EMPTY.get_or_init(|| Arc::new(Vec::new()))))
    }
}

/// Why a prompt's images cannot be sent. Each refusal names its fix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImageRefusal {
    Unsupported,
    TooMany,
    TooLarge,
}

impl ImageRefusal {
    pub(crate) fn message(self) -> &'static str {
        match self {
            Self::Unsupported => {
                "This harness does not advertise image prompt support. Remove the images or choose a harness that accepts them."
            }
            Self::TooMany => "A message can include at most four images. Remove some images.",
            Self::TooLarge => {
                "Images in one message can total at most 1 MiB after resizing to 1568 px. Remove some images or crop them."
            }
        }
    }
}

/// The count and total-size bound every image prompt meets, from `limits.rs`.
pub(crate) fn admit_sizes(sizes: impl IntoIterator<Item = usize>) -> Result<(), ImageRefusal> {
    let mut count = 0_usize;
    let mut total = 0_usize;
    for size in sizes {
        count += 1;
        total = total.saturating_add(size);
    }
    if count > PROMPT_IMAGE_COUNT {
        return Err(ImageRefusal::TooMany);
    }
    if total > PROMPT_IMAGE_TOTAL_BYTES {
        return Err(ImageRefusal::TooLarge);
    }
    Ok(())
}

impl PromptImages {
    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub(crate) fn retained_bytes(&self) -> usize {
        self.0.capacity() * std::mem::size_of::<PromptImage>()
            + self
                .0
                .iter()
                .map(|image| {
                    image.bytes.capacity()
                        + match &image.preview {
                            ImagePreview::Thumbnail(_, bytes) => bytes.capacity(),
                            _ => 0,
                        }
                })
                .sum::<usize>()
    }

    /// Standard ACP image content blocks, encoded only at the wire edge.
    pub(crate) fn wire(
        &self,
        capabilities: Option<&CapabilitySnapshotV1>,
    ) -> Result<Vec<serde_json::Value>, ImageRefusal> {
        if self.0.is_empty() {
            return Ok(Vec::new());
        }
        if !capabilities.is_some_and(|value| value.prompt_images) {
            return Err(ImageRefusal::Unsupported);
        }
        admit_sizes(self.0.iter().map(|image| image.bytes.len()))?;
        Ok(self
            .0
            .iter()
            .map(|image| {
                serde_json::json!({
                    "type": "image",
                    "mimeType": image.media_type.as_str(),
                    "data": STANDARD.encode(&image.bytes),
                })
            })
            .collect())
    }
}
