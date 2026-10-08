use std::io::Cursor;

use image::{
    DynamicImage, ImageFormat, ImageReader, Limits, codecs::jpeg::JpegEncoder, imageops::FilterType,
};
use sha2::{Digest, Sha256};

const MAX_INPUT_BYTES: usize = 8 * 1024 * 1024;
const MAX_INPUT_PIXELS: u64 = 16_000_000;
const MAX_DECODE_ALLOC: u64 = 64 * 1024 * 1024;
const MAX_EDGE: u32 = 2048;
const PNG_PREFERENCE_BYTES: usize = 2 * 1024 * 1024;
const MAX_OUTPUT_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NormalizedImage {
    pub bytes: Vec<u8>,
    pub media_type: &'static str,
    pub width: u32,
    pub height: u32,
    pub sha256: String,
}

pub(crate) struct Thumbnail {
    pub bytes: Vec<u8>,
    pub sha256: String,
}

pub(crate) fn make_thumbnail(image: &NormalizedImage) -> Result<Thumbnail, &'static str> {
    let decoded = image::load_from_memory(&image.bytes).map_err(|_| "image decoding failed")?;
    for edge in [256, 192, 128, 96, 64] {
        let scaled = decoded.thumbnail(edge, edge);
        let mut bytes = Vec::new();
        if image.media_type == "image/png" {
            let mut output = Cursor::new(Vec::new());
            scaled
                .write_to(&mut output, ImageFormat::Png)
                .map_err(|_| "thumbnail encoding failed")?;
            bytes = output.into_inner();
        } else {
            JpegEncoder::new_with_quality(&mut bytes, 75)
                .encode_image(&DynamicImage::ImageRgb8(scaled.to_rgb8()))
                .map_err(|_| "thumbnail encoding failed")?;
        }
        if !bytes.is_empty() && bytes.len() <= 262_144 {
            return Ok(Thumbnail {
                sha256: format!("{:x}", Sha256::digest(&bytes)),
                bytes,
            });
        }
    }
    Err("thumbnail exceeds the limit")
}

fn decoder(bytes: &[u8], format: ImageFormat) -> ImageReader<Cursor<&[u8]>> {
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(16_000);
    limits.max_image_height = Some(16_000);
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    reader.limits(limits);
    reader
}

pub(crate) fn normalize_image(bytes: &[u8]) -> Result<NormalizedImage, &'static str> {
    if bytes.is_empty() || bytes.len() > MAX_INPUT_BYTES {
        return Err("image encoded input exceeds the limit");
    }
    let format = image::guess_format(bytes).map_err(|_| "image format is unsupported")?;
    if !matches!(format, ImageFormat::Png | ImageFormat::Jpeg) {
        return Err("image format is unsupported");
    }
    let (source_width, source_height) = decoder(bytes, format)
        .into_dimensions()
        .map_err(|_| "image dimensions are unavailable")?;
    if source_width == 0
        || source_height == 0
        || u64::from(source_width) * u64::from(source_height) > MAX_INPUT_PIXELS
    {
        return Err("image dimensions exceed the limit");
    }
    let decoded = decoder(bytes, format)
        .decode()
        .map_err(|_| "image decoding failed")?;
    let normalized = if source_width > MAX_EDGE || source_height > MAX_EDGE {
        decoded.resize(MAX_EDGE, MAX_EDGE, FilterType::Lanczos3)
    } else {
        decoded
    };
    let has_alpha = normalized
        .to_rgba8()
        .pixels()
        .any(|pixel| pixel.0[3] != 255);
    let mut output = Cursor::new(Vec::new());
    normalized
        .write_to(&mut output, ImageFormat::Png)
        .map_err(|_| "image encoding failed")?;
    let mut output = output.into_inner();
    let media_type = if output.len() > PNG_PREFERENCE_BYTES && !has_alpha {
        let mut jpeg = Vec::new();
        JpegEncoder::new_with_quality(&mut jpeg, 85)
            .encode_image(&DynamicImage::ImageRgb8(normalized.to_rgb8()))
            .map_err(|_| "image encoding failed")?;
        output = jpeg;
        "image/jpeg"
    } else {
        "image/png"
    };
    if output.len() > MAX_OUTPUT_BYTES {
        return Err("normalized image exceeds the limit");
    }
    let sha256 = format!("{:x}", Sha256::digest(&output));
    Ok(NormalizedImage {
        bytes: output,
        media_type,
        width: normalized.width(),
        height: normalized.height(),
        sha256,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    fn png(width: u32, height: u32, pixel: Rgba<u8>) -> Vec<u8> {
        let mut output = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(width, height, pixel))
            .write_to(&mut output, ImageFormat::Png)
            .unwrap();
        output.into_inner()
    }

    #[test]
    fn normalizes_large_image_and_preserves_transparency() {
        let source = png(3000, 1000, Rgba([20, 40, 60, 70]));
        let image = normalize_image(&source).unwrap();
        assert_eq!((image.width, image.height), (2048, 683));
        assert_eq!(image.media_type, "image/png");
        assert_eq!(image.sha256.len(), 64);
        let decoded = image::load_from_memory(&image.bytes).unwrap();
        assert_eq!(decoded.to_rgba8().get_pixel(100, 100).0[3], 70);
    }

    #[test]
    fn rejects_unbounded_or_invalid_encoded_input() {
        assert!(normalize_image(&vec![0; MAX_INPUT_BYTES + 1]).is_err());
        assert!(normalize_image(b"not an image").is_err());
        assert!(normalize_image(&png(4097, 4097, Rgba([0, 0, 0, 255]))).is_err());
    }

    #[test]
    fn opaque_rgba_png_uses_jpeg_when_png_exceeds_two_megabytes() {
        let mut pixels = RgbaImage::new(1024, 1024);
        let mut random = 0x1234_5678_u32;
        for pixel in pixels.pixels_mut() {
            let mut rgb = [0_u8; 3];
            for channel in &mut rgb {
                random ^= random << 13;
                random ^= random >> 17;
                random ^= random << 5;
                *channel = random as u8;
            }
            *pixel = Rgba([rgb[0], rgb[1], rgb[2], 255]);
        }
        let mut encoded = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(pixels)
            .write_to(&mut encoded, ImageFormat::Png)
            .unwrap();
        let normalized = normalize_image(&encoded.into_inner()).unwrap();
        assert_eq!(normalized.media_type, "image/jpeg");
        assert!(normalized.bytes.len() <= MAX_OUTPUT_BYTES);
        assert_eq!(
            image::guess_format(&normalized.bytes).unwrap(),
            ImageFormat::Jpeg
        );
    }

    #[test]
    fn thumbnail_is_bounded_and_hashes_its_encoded_bytes() {
        let image = normalize_image(&png(1024, 768, Rgba([20, 40, 60, 70]))).unwrap();
        let thumbnail = make_thumbnail(&image).unwrap();
        assert!(thumbnail.bytes.len() <= 262_144);
        assert_eq!(
            thumbnail.sha256,
            format!("{:x}", Sha256::digest(&thumbnail.bytes))
        );
        let decoded = image::load_from_memory(&thumbnail.bytes).unwrap();
        assert!(decoded.width() <= 256 && decoded.height() <= 256);
    }
}
