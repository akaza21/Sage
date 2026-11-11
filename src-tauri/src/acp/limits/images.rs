//! Attached image bounds, from the file a user picks to the frame a prompt
//! sends. Every number below is either a product decision documented where it
//! is set or derived from the transport limits it must fit.
use super::LIMITS;
use crate::acp::service::prompt_files::{DISPATCH_BYTES, TOTAL_BYTES as PROMPT_FILE_BYTES};

/// Images one prompt may carry; the composer attaches at most this many.
pub const PROMPT_IMAGE_COUNT: usize = 4;

/// An attached file as the user supplies it, before normalisation: a 4K or 5K
/// screenshot or a phone photo fits. Its header is checked against the pixel
/// bounds before any pixel is decoded.
pub const IMAGE_UPLOAD_BYTES: usize = 10 * 1024 * 1024;
pub const IMAGE_UPLOAD_EDGE_PX: u32 = 8192;
pub const IMAGE_UPLOAD_PIXELS: u64 = 16_777_216;
/// Native bytes a pixel while a progressive JPEG decodes (jpeg-decoder 0.3):
/// 2-byte coefficients for three components (6), the three component planes
/// (3) and the RGB output (3), all live at once. This is the worst decode;
/// a PNG decodes straight into its 4-byte RGBA raster.
pub const IMAGE_DECODE_BYTES_PER_PIXEL: u64 = 6 + 3 + 3;
/// Headroom the PNG decoder may take beside its raster for row buffers.
pub const PNG_DECODER_ROW_BYTES: usize = 1024 * 1024;
/// An upper bound on native memory while normalising one image (about 202
/// MiB at the pixel bound). The encoded file stays alive throughout (its
/// base64 is freed first); on top of it the larger of:
/// - the decode above;
/// - widening the decoded pixels to RGBA (3 + 4 bytes a pixel while it moves);
/// - the full raster beside its downscale, its upright copy and their
///   encodings, each at most the stored edge.
pub const IMAGE_NORMALIZE_PEAK_BYTES: u64 = IMAGE_UPLOAD_BYTES as u64
    + max(
        IMAGE_UPLOAD_PIXELS * IMAGE_DECODE_BYTES_PER_PIXEL,
        IMAGE_UPLOAD_PIXELS * 4 + STORED_IMAGE_PIXELS * 4 * 4,
    );
/// Native memory all image normalisation may hold at once, uploads and
/// prompt sends together.
pub const IMAGE_WORK_MEMORY_BYTES: u64 = 256 * 1024 * 1024;
/// Normalisations that may run at once: as many peaks as fit the ceiling.
pub const IMAGE_WORK_PERMITS: usize =
    (IMAGE_WORK_MEMORY_BYTES / IMAGE_NORMALIZE_PEAK_BYTES) as usize;

const fn max(left: u64, right: u64) -> u64 {
    if left > right { left } else { right }
}

/// What the image store keeps for any harness. An upload past these bounds
/// is normalised down to them; one within them is kept byte for byte.
pub const STORED_IMAGE_BYTES: usize = 4 * 1024 * 1024;
pub const STORED_IMAGE_EDGE_PX: u32 = 2048;
pub const STORED_IMAGE_PIXELS: u64 = STORED_IMAGE_EDGE_PX as u64 * STORED_IMAGE_EDGE_PX as u64;

/// The long edge an image is normalised to for an ACP image prompt. Claude's
/// vision input resizes anything longer than about 1568 px, so more pixels
/// carry nothing.
pub const PROMPT_IMAGE_EDGE_PX: u32 = 1568;
/// A normalised image stays PNG (lossless, best for screenshots) up to this
/// size; past it the smaller of PNG and a quality-85 JPEG is kept. A typical
/// 1568 px UI screenshot is 150-500 KiB as PNG.
pub const PROMPT_IMAGE_PNG_BYTES: usize = 512 * 1024;
pub const PROMPT_IMAGE_JPEG_QUALITY: u8 = 85;
/// Decoded bytes of every normalised image in one prompt together, and so of
/// any one image: three or four normalised screenshots.
pub const PROMPT_IMAGE_TOTAL_BYTES: usize = 1024 * 1024;
/// Codex and Pi repeat each prompt image inside their own events, as base64
/// in one inbound JSON line that must fit the ordinary frame. The images of
/// one prompt to them, and so any one image, total at most this.
pub const ECHOED_PROMPT_IMAGE_TOTAL_BYTES: usize = 600 * 1024;
/// The long edge of the transcript copy of a sent image too large to keep
/// whole: legible in the transcript, a quarter of the prompt's pixels.
pub const IMAGE_PREVIEW_EDGE_PX: u32 = 768;
/// The long edge of the store's own thumbnail of a large image.
pub const STORED_THUMBNAIL_EDGE_PX: u32 = 128;
/// JSON escapes a byte to at most six (a control character as `\u001b`).
pub const JSON_ESCAPE_MAX_BYTES: usize = 6;

const fn base64_len(bytes: usize) -> usize {
    bytes.div_ceil(3) * 4
}

/// The JSON-RPC envelope of a `session/prompt` frame: ids, keys, MIME types
/// and the per-block base64 padding of every image and file.
pub const PROMPT_FRAME_ENVELOPE_BYTES: usize = 4096;
/// The one outbound frame class larger than `WireLimits::frame_bytes`: an
/// ACP `session/prompt` request carrying images. It is what the largest prompt
/// needs: the images as base64, plus the prompt text and staged files that
/// dispatch admits (`DISPATCH_BYTES`) with every byte escaped at its worst.
/// Inbound frames keep the ordinary bound.
pub const PROMPT_FRAME_BYTES: usize = base64_len(PROMPT_IMAGE_TOTAL_BYTES)
    + JSON_ESCAPE_MAX_BYTES * DISPATCH_BYTES
    + PROMPT_FRAME_ENVELOPE_BYTES;
/// Egress bytes held back for one queued prompt frame and its delimiter, on
/// top of the ordinary data lane, so a prompt is never refused because other
/// data frames are queued.
pub const PROMPT_EGRESS_RESERVE_BYTES: usize = PROMPT_FRAME_BYTES + 1;

/// The JSON envelope of one normalized image item around its base64 data.
pub const NORMALIZED_IMAGE_ENVELOPE_BYTES: usize = 256;
/// Decoded bytes of the transcript copy of one sent image: the most whose
/// base64 still fits one normalized item with its envelope.
pub const IMAGE_PREVIEW_BYTES: usize =
    (LIMITS.wire.normalized_item_bytes as usize - NORMALIZED_IMAGE_ENVELOPE_BYTES) / 4 * 3;
/// A prompt intent's own fields beside its text, files, images and previews.
pub const PROMPT_INTENT_OVERHEAD_BYTES: usize = 8192;

const _: () = assert!(
    PROMPT_FRAME_BYTES > LIMITS.wire.frame_bytes as usize
        && PROMPT_IMAGE_PNG_BYTES <= PROMPT_IMAGE_TOTAL_BYTES
        && ECHOED_PROMPT_IMAGE_TOTAL_BYTES <= PROMPT_IMAGE_TOTAL_BYTES
        && PROMPT_IMAGE_PNG_BYTES <= ECHOED_PROMPT_IMAGE_TOTAL_BYTES
        // The echo of every image leaves room for the prompt text; a text
        // whose escaped echo would not fit is refused when the turn is sent.
        && base64_len(ECHOED_PROMPT_IMAGE_TOTAL_BYTES) + DISPATCH_BYTES + PROMPT_FRAME_ENVELOPE_BYTES
            <= LIMITS.wire.frame_bytes as usize
        && PROMPT_IMAGE_TOTAL_BYTES <= STORED_IMAGE_BYTES
        && PROMPT_IMAGE_EDGE_PX <= STORED_IMAGE_EDGE_PX
        && STORED_IMAGE_BYTES <= IMAGE_UPLOAD_BYTES
        && STORED_IMAGE_PIXELS <= IMAGE_UPLOAD_PIXELS
        && IMAGE_PREVIEW_EDGE_PX <= PROMPT_IMAGE_EDGE_PX
        // Staged files are part of the dispatch budget the frame is sized by.
        && PROMPT_FILE_BYTES <= DISPATCH_BYTES
        // At least one normalisation fits the ceiling, and all that run fit it.
        && IMAGE_WORK_PERMITS >= 1
        && IMAGE_WORK_PERMITS as u64 * IMAGE_NORMALIZE_PEAK_BYTES <= IMAGE_WORK_MEMORY_BYTES
        // A prompt with every image, its previews, text and files fits the
        // process driver's data lane.
        && PROMPT_IMAGE_TOTAL_BYTES
            + PROMPT_IMAGE_COUNT * IMAGE_PREVIEW_BYTES
            + DISPATCH_BYTES
            + PROMPT_INTENT_OVERHEAD_BYTES
            < LIMITS.queues.driver_data_bytes as usize
        && base64_len(IMAGE_PREVIEW_BYTES) + NORMALIZED_IMAGE_ENVELOPE_BYTES
            <= LIMITS.wire.normalized_item_bytes as usize
);

#[cfg(test)]
mod tests {
    use super::*;

    /// The value of `export const NAME = a * b * c;` in the frontend mirror.
    fn mirrored(name: &str) -> usize {
        let source = include_str!("../../../../src/domain/attachedContext.ts");
        let prefix = format!("export const {name} = ");
        let line = source
            .lines()
            .find_map(|line| line.strip_prefix(prefix.as_str()))
            .unwrap_or_else(|| panic!("{name} is not mirrored"));
        line.trim_end_matches(';')
            .split('*')
            .map(|factor| factor.trim().replace('_', "").parse::<usize>().unwrap())
            .product()
    }

    #[test]
    fn the_frontend_mirrors_the_image_limits() {
        assert_eq!(mirrored("MAX_OUTGOING_IMAGE_BYTES"), STORED_IMAGE_BYTES);
        assert_eq!(mirrored("MAX_PROMPT_IMAGES"), PROMPT_IMAGE_COUNT);
        assert_eq!(mirrored("MAX_IMAGE_UPLOAD_BYTES"), IMAGE_UPLOAD_BYTES);
    }
}
