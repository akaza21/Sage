//! Attached images are normalised natively. Within the bounds of their use an
//! image keeps its original bytes; otherwise it is decoded, area-averaged down
//! to the long edge with premultiplied alpha, turned upright from its EXIF
//! orientation, and re-encoded as PNG, or as a JPEG when that is what fits.
//! The store applies `STORED` for every harness; an ACP image prompt applies
//! the tighter `PROMPT` bounds when it is sent.
use std::io::Cursor;

use crate::{
    acp::{
        intent::ImageMediaType,
        limits::{
            IMAGE_PREVIEW_BYTES, IMAGE_PREVIEW_EDGE_PX, IMAGE_UPLOAD_BYTES, IMAGE_UPLOAD_EDGE_PX,
            IMAGE_UPLOAD_PIXELS, PNG_DECODER_ROW_BYTES, PROMPT_IMAGE_EDGE_PX,
            PROMPT_IMAGE_JPEG_QUALITY, PROMPT_IMAGE_PNG_BYTES, PROMPT_IMAGE_TOTAL_BYTES,
            STORED_IMAGE_BYTES, STORED_IMAGE_EDGE_PX,
        },
    },
    error::{AppError, AppResult},
};

/// Where a normalised image goes: its long edge, its byte bound, and the size
/// up to which lossless PNG is kept before a smaller JPEG is preferred.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Bounds {
    pub edge: u32,
    pub bytes: usize,
    pub png_bytes: usize,
    /// Whether an image that is already in bounds but carries an EXIF
    /// rotation is re-encoded upright, for a reader that may ignore EXIF.
    pub upright: bool,
}

/// What the image store keeps for every harness.
pub(crate) const STORED: Bounds = Bounds {
    edge: STORED_IMAGE_EDGE_PX,
    bytes: STORED_IMAGE_BYTES,
    png_bytes: STORED_IMAGE_BYTES,
    upright: false,
};
/// What an ACP image prompt sends.
pub(crate) const PROMPT: Bounds = Bounds {
    edge: PROMPT_IMAGE_EDGE_PX,
    bytes: PROMPT_IMAGE_TOTAL_BYTES,
    png_bytes: PROMPT_IMAGE_PNG_BYTES,
    upright: true,
};
/// The transcript copy of a sent image too large to keep whole.
pub(crate) const PREVIEW: Bounds = Bounds {
    edge: IMAGE_PREVIEW_EDGE_PX,
    bytes: IMAGE_PREVIEW_BYTES,
    png_bytes: IMAGE_PREVIEW_BYTES,
    upright: true,
};

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Normalized {
    pub media_type: ImageMediaType,
    pub bytes: Vec<u8>,
}

/// Decoded RGBA8 pixels.
struct Raster {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

/// What a header says before any pixel is decoded.
struct Header {
    width: u32,
    height: u32,
    orientation: u8,
}

fn invalid(message: impl Into<String>) -> AppError {
    AppError::InvalidRequest(message.into())
}

fn malformed() -> AppError {
    invalid("image data is corrupt or truncated; export it again as PNG or JPEG")
}

/// `bytes` as KiB or MiB for a user-facing message.
pub(crate) fn byte_size(bytes: usize) -> String {
    if bytes.is_multiple_of(1024 * 1024) {
        format!("{} MiB", bytes / (1024 * 1024))
    } else {
        format!("{} KiB", bytes / 1024)
    }
}

/// The form `bounds` admits, or a refusal that names its limit and its fix.
pub(crate) fn normalize(
    media_type: ImageMediaType,
    bytes: &[u8],
    bounds: Bounds,
) -> AppResult<Normalized> {
    if bytes.is_empty() || bytes.len() > IMAGE_UPLOAD_BYTES {
        return Err(invalid(format!(
            "image is larger than the {} upload limit",
            byte_size(IMAGE_UPLOAD_BYTES)
        )));
    }
    let header = header(media_type, bytes)?;
    let upright = header.orientation <= 1 || !bounds.upright;
    if header.width.max(header.height) <= bounds.edge && bytes.len() <= bounds.bytes && upright {
        return Ok(Normalized {
            media_type,
            bytes: bytes.to_vec(),
        });
    }
    let raster = decode(media_type, bytes)?;
    let scaled = orient(downscale(raster, bounds.edge), header.orientation);
    let opaque = scaled.pixels.chunks_exact(4).all(|pixel| pixel[3] == 255);
    let png = encode_png(&scaled, opaque)?;
    if png.len() <= bounds.png_bytes.min(bounds.bytes) {
        return Ok(Normalized {
            media_type: ImageMediaType::Png,
            bytes: png,
        });
    }
    let jpeg = encode_jpeg(&scaled)?;
    let (media_type, bytes) = if jpeg.len() < png.len() {
        (ImageMediaType::Jpeg, jpeg)
    } else {
        (ImageMediaType::Png, png)
    };
    if bytes.len() > bounds.bytes {
        return Err(AppError::ResourceLimit(format!(
            "image is still larger than {} after resizing to {} px; crop it or attach a smaller image",
            byte_size(bounds.bytes),
            bounds.edge
        )));
    }
    Ok(Normalized { media_type, bytes })
}

fn admit_dimensions(width: u32, height: u32) -> AppResult<()> {
    if width == 0 || height == 0 {
        return Err(malformed());
    }
    if width > IMAGE_UPLOAD_EDGE_PX
        || height > IMAGE_UPLOAD_EDGE_PX
        || u64::from(width) * u64::from(height) > IMAGE_UPLOAD_PIXELS
    {
        return Err(invalid(format!(
            "image is {width}x{height} px; images can be at most {IMAGE_UPLOAD_EDGE_PX} px on a side and {} megapixels",
            IMAGE_UPLOAD_PIXELS / (1024 * 1024)
        )));
    }
    Ok(())
}

/// The header's dimensions, checked against the upload bounds, without
/// decoding any pixel.
pub(crate) fn dimensions(media_type: ImageMediaType, bytes: &[u8]) -> AppResult<(u32, u32)> {
    let header = header(media_type, bytes)?;
    Ok((header.width, header.height))
}

fn header(media_type: ImageMediaType, bytes: &[u8]) -> AppResult<Header> {
    let header = match media_type {
        // IHDR is the first chunk: signature, length, type, width, height.
        ImageMediaType::Png => {
            if bytes.len() < 24
                || !bytes.starts_with(b"\x89PNG\r\n\x1a\n")
                || &bytes[12..16] != b"IHDR"
            {
                return Err(malformed());
            }
            let word = |at: usize| {
                u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
            };
            Header {
                width: word(16),
                height: word(20),
                orientation: 1,
            }
        }
        ImageMediaType::Jpeg => {
            let mut decoder = jpeg_decoder::Decoder::new(Cursor::new(bytes));
            decoder.read_info().map_err(|_| malformed())?;
            let info = decoder.info().ok_or_else(malformed)?;
            Header {
                width: u32::from(info.width),
                height: u32::from(info.height),
                orientation: decoder.exif_data().map_or(1, exif_orientation),
            }
        }
        ImageMediaType::Webp => {
            return Err(invalid("WebP images are unsupported; attach PNG or JPEG"));
        }
    };
    admit_dimensions(header.width, header.height)?;
    Ok(header)
}

/// The EXIF orientation tag (0x0112) of a TIFF block, or 1 (upright).
fn exif_orientation(tiff: &[u8]) -> u8 {
    let big = match tiff.get(..4) {
        Some(b"MM\0*") => true,
        Some(b"II*\0") => false,
        _ => return 1,
    };
    let read = |at: usize, width: usize| -> Option<u32> {
        let slice = tiff.get(at..at + width)?;
        Some(slice.iter().enumerate().fold(0, |value, (index, byte)| {
            let shift = if big { width - 1 - index } else { index } * 8;
            value | u32::from(*byte) << shift
        }))
    };
    let Some(ifd) = read(4, 4).map(|offset| offset as usize) else {
        return 1;
    };
    let count = read(ifd, 2).unwrap_or(0) as usize;
    (0..count)
        .map(|index| ifd + 2 + index * 12)
        .find(|entry| read(*entry, 2) == Some(0x0112))
        .and_then(|entry| read(entry + 8, 2))
        .filter(|value| (1..=8).contains(value))
        .map_or(1, |value| value as u8)
}

fn decode(media_type: ImageMediaType, bytes: &[u8]) -> AppResult<Raster> {
    match media_type {
        ImageMediaType::Png => decode_png(bytes),
        ImageMediaType::Jpeg => decode_jpeg(bytes),
        ImageMediaType::Webp => Err(invalid("WebP images are unsupported; attach PNG or JPEG")),
    }
}

fn decode_png(bytes: &[u8]) -> AppResult<Raster> {
    let mut decoder = png::Decoder::new_with_limits(
        Cursor::new(bytes),
        png::Limits {
            // The RGBA raster plus the decoder's row buffers.
            bytes: IMAGE_UPLOAD_PIXELS as usize * 4 + PNG_DECODER_ROW_BYTES,
        },
    );
    decoder.set_ignore_text_chunk(true);
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|_| malformed())?;
    let info = reader.info();
    let (width, height) = (info.width, info.height);
    admit_dimensions(width, height)?;
    if info.animation_control.is_some() {
        return Err(invalid(
            "animated PNG images are unsupported; attach a single frame",
        ));
    }
    let count = width as usize * height as usize;
    // Room for RGBA up front, so widening never reallocates.
    let mut pixels = Vec::with_capacity(count * 4);
    pixels.resize(reader.output_buffer_size(), 0);
    let frame = reader.next_frame(&mut pixels).map_err(|_| malformed())?;
    let channels = match frame.color_type {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => return Err(malformed()),
    };
    pixels.truncate(frame.buffer_size());
    Ok(widen(width, height, pixels, channels))
}

fn decode_jpeg(bytes: &[u8]) -> AppResult<Raster> {
    let mut decoder = jpeg_decoder::Decoder::new(Cursor::new(bytes));
    decoder.read_info().map_err(|_| malformed())?;
    let info = decoder.info().ok_or_else(malformed)?;
    let (width, height) = (u32::from(info.width), u32::from(info.height));
    admit_dimensions(width, height)?;
    let channels = match info.pixel_format {
        jpeg_decoder::PixelFormat::L8 => 1,
        jpeg_decoder::PixelFormat::RGB24 => 3,
        jpeg_decoder::PixelFormat::L16 => {
            return Err(invalid(
                "16-bit JPEG images are unsupported; save it as an 8-bit JPEG or PNG",
            ));
        }
        jpeg_decoder::PixelFormat::CMYK32 => {
            return Err(invalid(
                "CMYK JPEG images are unsupported; save it as an RGB JPEG or PNG",
            ));
        }
    };
    let decoded = decoder.decode().map_err(|_| malformed())?;
    // The coefficient buffers go with the decoder before widening.
    drop(decoder);
    if decoded.len() != width as usize * height as usize * channels {
        return Err(malformed());
    }
    Ok(widen(width, height, decoded, channels))
}

/// Widens 1-4 channel pixels to RGBA in place, from the back, so no second
/// raster is allocated.
fn widen(width: u32, height: u32, mut pixels: Vec<u8>, channels: usize) -> Raster {
    let count = width as usize * height as usize;
    if channels != 4 {
        pixels.resize(count * 4, 0);
        for index in (0..count).rev() {
            let (source, target) = (index * channels, index * 4);
            let pixel = match channels {
                1 => [pixels[source], pixels[source], pixels[source], 255],
                2 => {
                    let grey = pixels[source];
                    [grey, grey, grey, pixels[source + 1]]
                }
                _ => [pixels[source], pixels[source + 1], pixels[source + 2], 255],
            };
            pixels[target..target + 4].copy_from_slice(&pixel);
        }
    }
    Raster {
        width,
        height,
        pixels,
    }
}

/// Area-average downscale over premultiplied alpha, so text and thin UI lines
/// stay legible and transparent pixels lend no colour to their neighbours.
fn downscale(raster: Raster, edge: u32) -> Raster {
    let long = raster.width.max(raster.height);
    if long <= edge {
        return raster;
    }
    let scale = |value: u32| (u64::from(value) * u64::from(edge)).div_ceil(u64::from(long)) as u32;
    let (width, height) = (scale(raster.width).max(1), scale(raster.height).max(1));
    let (source_width, source_height) = (raster.width as usize, raster.height as usize);
    let mut pixels = vec![0_u8; width as usize * height as usize * 4];
    for y in 0..height as usize {
        let top = y * source_height / height as usize;
        let bottom = ((y + 1) * source_height / height as usize).max(top + 1);
        for x in 0..width as usize {
            let left = x * source_width / width as usize;
            let right = ((x + 1) * source_width / width as usize).max(left + 1);
            let mut sum = [0_u64; 4];
            for row in top..bottom {
                let start = (row * source_width + left) * 4;
                for pixel in raster.pixels[start..start + (right - left) * 4].chunks_exact(4) {
                    let alpha = u64::from(pixel[3]);
                    for channel in 0..3 {
                        sum[channel] += u64::from(pixel[channel]) * alpha;
                    }
                    sum[3] += alpha;
                }
            }
            let count = ((bottom - top) * (right - left)) as u64;
            let target = (y * width as usize + x) * 4;
            // Fully transparent areas keep zero colour.
            for channel in 0..3 {
                pixels[target + channel] =
                    (sum[channel] + sum[3] / 2).checked_div(sum[3]).unwrap_or(0) as u8;
            }
            pixels[target + 3] = ((sum[3] + count / 2) / count) as u8;
        }
    }
    Raster {
        width,
        height,
        pixels,
    }
}

/// Applies an EXIF orientation (2-8) so the raster is upright.
fn orient(raster: Raster, orientation: u8) -> Raster {
    if !(2..=8).contains(&orientation) {
        return raster;
    }
    let (w, h) = (raster.width as usize, raster.height as usize);
    let swap = orientation >= 5;
    let (width, height) = if swap { (h, w) } else { (w, h) };
    let mut pixels = vec![0_u8; raster.pixels.len()];
    for y in 0..height {
        for x in 0..width {
            let (sx, sy) = match orientation {
                2 => (w - 1 - x, y),
                3 => (w - 1 - x, h - 1 - y),
                4 => (x, h - 1 - y),
                5 => (y, x),
                6 => (y, h - 1 - x),
                7 => (w - 1 - y, h - 1 - x),
                _ => (w - 1 - y, x),
            };
            let (source, target) = ((sy * w + sx) * 4, (y * width + x) * 4);
            pixels[target..target + 4].copy_from_slice(&raster.pixels[source..source + 4]);
        }
    }
    Raster {
        width: width as u32,
        height: height as u32,
        pixels,
    }
}

fn encode_png(raster: &Raster, opaque: bool) -> AppResult<Vec<u8>> {
    let rgb;
    let (color, data) = if opaque {
        rgb = raster
            .pixels
            .chunks_exact(4)
            .flat_map(|pixel| [pixel[0], pixel[1], pixel[2]])
            .collect::<Vec<_>>();
        (png::ColorType::Rgb, rgb.as_slice())
    } else {
        (png::ColorType::Rgba, raster.pixels.as_slice())
    };
    let mut output = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut output, raster.width, raster.height);
        encoder.set_color(color);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Best);
        encoder.set_adaptive_filter(png::AdaptiveFilterType::Adaptive);
        encoder
            .write_header()
            .and_then(|mut writer| writer.write_image_data(data))
            .map_err(|_| invalid("image could not be re-encoded"))?;
    }
    // The encoder grows its output by doubling, so a PNG can end with twice
    // its length allocated; a prompt is charged what it holds.
    output.shrink_to_fit();
    Ok(output)
}

/// JPEG has no alpha; transparent areas are composed onto white.
fn encode_jpeg(raster: &Raster) -> AppResult<Vec<u8>> {
    let rgb = raster
        .pixels
        .chunks_exact(4)
        .flat_map(|pixel| {
            let alpha = u16::from(pixel[3]);
            let blend = |channel: u8| {
                ((u16::from(channel) * alpha + 255 * (255 - alpha) + 127) / 255) as u8
            };
            [blend(pixel[0]), blend(pixel[1]), blend(pixel[2])]
        })
        .collect::<Vec<_>>();
    let (width, height) = (
        u16::try_from(raster.width).map_err(|_| invalid("image could not be re-encoded"))?,
        u16::try_from(raster.height).map_err(|_| invalid("image could not be re-encoded"))?,
    );
    let mut output = Vec::new();
    jpeg_encoder::Encoder::new(&mut output, PROMPT_IMAGE_JPEG_QUALITY)
        .encode(&rgb, width, height, jpeg_encoder::ColorType::Rgb)
        .map_err(|_| invalid("image could not be re-encoded"))?;
    output.shrink_to_fit();
    Ok(output)
}

#[cfg(test)]
#[path = "normalize_tests.rs"]
mod tests;
