//! Images a native prompt carries as standard ACP image content blocks. The
//! bytes come from the owner-bound image artifact store and live only for one
//! dispatch; nothing here is persisted except each transcript preview.
use std::sync::Arc;

use base64::{Engine as _, engine::general_purpose::STANDARD};

use crate::acp::intent::{CapabilitySnapshotV1, ImageMediaType};
use sage_core::limits::{PROMPT_IMAGE_COUNT, PROMPT_IMAGE_EDGE_PX, PROMPT_IMAGE_TOTAL_BYTES};

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
    /// The images total more than this many bytes.
    TooLarge(usize),
}

impl ImageRefusal {
    /// The user-facing refusal, built from the `limits.rs` numbers.
    pub(crate) fn message(self) -> String {
        match self {
            Self::Unsupported => "This harness does not advertise image prompt support. Remove the images or choose a harness that accepts them.".into(),
            Self::TooMany => format!(
                "A message can include at most {PROMPT_IMAGE_COUNT} images. Remove some images."
            ),
            Self::TooLarge(total) => format!(
                "Images in one message can total at most {} after resizing to {PROMPT_IMAGE_EDGE_PX} px. Remove some images or crop them.",
                crate::image_artifacts::byte_size(total)
            ),
        }
    }
}

/// The count and total-size bound every image prompt meets, from `limits.rs`.
pub(crate) fn admit_sizes(
    sizes: impl IntoIterator<Item = usize>,
    total_bytes: usize,
) -> Result<(), ImageRefusal> {
    let mut count = 0_usize;
    let mut total = 0_usize;
    for size in sizes {
        count += 1;
        total = total.saturating_add(size);
    }
    if count > PROMPT_IMAGE_COUNT {
        return Err(ImageRefusal::TooMany);
    }
    if total > total_bytes {
        return Err(ImageRefusal::TooLarge(total_bytes));
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
        admit_sizes(
            self.0.iter().map(|image| image.bytes.len()),
            PROMPT_IMAGE_TOTAL_BYTES,
        )?;
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

    /// Each image's media type and base64, for a harness that repeats its
    /// prompt images in its own events and so takes the echoed bound.
    pub(crate) fn echoed(&self) -> Result<Vec<(&'static str, String)>, ImageRefusal> {
        admit_sizes(
            self.0.iter().map(|image| image.bytes.len()),
            sage_core::limits::ECHOED_PROMPT_IMAGE_TOTAL_BYTES,
        )?;
        Ok(self
            .0
            .iter()
            .map(|image| (image.media_type.as_str(), STANDARD.encode(&image.bytes)))
            .collect())
    }

    /// Whether a harness that echoes the prompt's text and every image in
    /// one inbound line can fit that echo in a frame: text that escapes
    /// heavily could push it past.
    pub(crate) fn echo_fits(&self, text: &str) -> bool {
        if self.0.is_empty() {
            return true;
        }
        let escaped = serde_json::to_string(text).map_or(usize::MAX, |text| text.len());
        let images = self
            .0
            .iter()
            .map(|image| image.bytes.len().div_ceil(3) * 4)
            .sum::<usize>();
        escaped.saturating_add(images + sage_core::limits::PROMPT_FRAME_ENVELOPE_BYTES)
            <= sage_core::limits::LIMITS.wire.frame_bytes as usize
    }
}
