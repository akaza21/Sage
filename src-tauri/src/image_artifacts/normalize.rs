//! Attached images are normalised natively before they are stored, so what a
//! prompt sends is what a vision model can use: at most `PROMPT_IMAGE_EDGE_PX`
//! on the long edge, kept as the original bytes when already small enough,
//! otherwise downscaled with an area average and re-encoded as PNG, or as a
//! quality-85 JPEG when that is what fits.
use std::io::Cursor;

use crate::{
    acp::{
        intent::ImageMediaType,
        limits::{
            IMAGE_UPLOAD_BYTES, IMAGE_UPLOAD_EDGE_PX, IMAGE_UPLOAD_PIXELS, PROMPT_IMAGE_EDGE_PX,
            PROMPT_IMAGE_PNG_BYTES, PROMPT_IMAGE_TOTAL_BYTES,
        },
    },
    error::{AppError, AppResult},
};

const JPEG_QUALITY: u8 = 85;

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

fn invalid(message: &str) -> AppError {
    AppError::InvalidRequest(message.into())
}

/// The prompt form of an attached image, or a refusal that says why.
pub(crate) fn normalize_for_prompt(
    media_type: ImageMediaType,
    bytes: &[u8],
) -> AppResult<Normalized> {
    if bytes.is_empty() || bytes.len() > IMAGE_UPLOAD_BYTES {
        return Err(invalid("image exceeds the 10 MiB upload bound"));
    }
    normalize(
        media_type,
        bytes,
        PROMPT_IMAGE_EDGE_PX,
        PROMPT_IMAGE_TOTAL_BYTES,
    )
    .map_err(|error| match error {
        AppError::ResourceLimit(_) => AppError::ResourceLimit(
            "image is still larger than 1 MiB after resizing to 1568 px; crop it or attach a smaller image".into(),
        ),
        other => other,
    })
}

/// A transcript-sized copy of an already normalised image.
pub(crate) fn preview(
    media_type: ImageMediaType,
    bytes: &[u8],
    edge: u32,
    max_bytes: usize,
) -> AppResult<Normalized> {
    normalize(media_type, bytes, edge, max_bytes)
}

fn normalize(
    media_type: ImageMediaType,
    bytes: &[u8],
    edge: u32,
    max_bytes: usize,
) -> AppResult<Normalized> {
    // Within bounds, the original bytes are kept without decoding them.
    let (width, height) = dimensions(media_type, bytes)?;
    if width.max(height) <= edge && bytes.len() <= max_bytes {
        return Ok(Normalized {
            media_type,
            bytes: bytes.to_vec(),
        });
    }
    let raster = decode(media_type, bytes)?;
    let scaled = downscale(raster, edge);
    let opaque = scaled.pixels.chunks_exact(4).all(|pixel| pixel[3] == 255);
    let png = encode_png(&scaled, opaque)?;
    if png.len() <= PROMPT_IMAGE_PNG_BYTES.min(max_bytes) {
        return Ok(Normalized {
            media_type: ImageMediaType::Png,
            bytes: png,
        });
    }
    let jpeg = encode_jpeg(&scaled)?;
    let best = if jpeg.len() < png.len() {
        Normalized {
            media_type: ImageMediaType::Jpeg,
            bytes: jpeg,
        }
    } else {
        Normalized {
            media_type: ImageMediaType::Png,
            bytes: png,
        }
    };
    if best.bytes.len() > max_bytes {
        return Err(AppError::ResourceLimit("normalised image".into()));
    }
    Ok(best)
}

fn admit_dimensions(width: u32, height: u32) -> AppResult<()> {
    if width == 0
        || height == 0
        || width > IMAGE_UPLOAD_EDGE_PX
        || height > IMAGE_UPLOAD_EDGE_PX
        || u64::from(width) * u64::from(height) > IMAGE_UPLOAD_PIXELS
    {
        return Err(invalid("image dimensions are unsupported"));
    }
    Ok(())
}

/// The header's dimensions, without decoding pixels.
pub(crate) fn dimensions(media_type: ImageMediaType, bytes: &[u8]) -> AppResult<(u32, u32)> {
    let (width, height) = match media_type {
        ImageMediaType::Png => {
            let reader = png::Decoder::new(Cursor::new(bytes))
                .read_info()
                .map_err(|_| invalid("image is malformed"))?;
            (reader.info().width, reader.info().height)
        }
        ImageMediaType::Jpeg => {
            let mut decoder = jpeg_decoder::Decoder::new(Cursor::new(bytes));
            decoder
                .read_info()
                .map_err(|_| invalid("image is malformed"))?;
            let info = decoder
                .info()
                .ok_or_else(|| invalid("image is malformed"))?;
            (u32::from(info.width), u32::from(info.height))
        }
        ImageMediaType::Webp => return Err(invalid("outgoing WebP is unsupported")),
    };
    admit_dimensions(width, height)?;
    Ok((width, height))
}

fn decode(media_type: ImageMediaType, bytes: &[u8]) -> AppResult<Raster> {
    match media_type {
        ImageMediaType::Png => decode_png(bytes),
        ImageMediaType::Jpeg => decode_jpeg(bytes),
        ImageMediaType::Webp => Err(invalid("outgoing WebP is unsupported")),
    }
}

fn decode_png(bytes: &[u8]) -> AppResult<Raster> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_ignore_text_chunk(true);
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder
        .read_info()
        .map_err(|_| invalid("image is malformed"))?;
    let info = reader.info();
    let (width, height) = (info.width, info.height);
    admit_dimensions(width, height)?;
    if info.animation_control.is_some() {
        return Err(invalid("animated images are unsupported"));
    }
    let mut decoded = vec![0; reader.output_buffer_size()];
    let frame = reader
        .next_frame(&mut decoded)
        .map_err(|_| invalid("image is malformed"))?;
    let channels = match frame.color_type {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => return Err(invalid("image color format is unsupported")),
    };
    Ok(to_rgba(
        width,
        height,
        &decoded[..frame.buffer_size()],
        channels,
    ))
}

fn decode_jpeg(bytes: &[u8]) -> AppResult<Raster> {
    let mut decoder = jpeg_decoder::Decoder::new(Cursor::new(bytes));
    decoder
        .read_info()
        .map_err(|_| invalid("image is malformed"))?;
    let info = decoder
        .info()
        .ok_or_else(|| invalid("image is malformed"))?;
    let (width, height) = (u32::from(info.width), u32::from(info.height));
    admit_dimensions(width, height)?;
    let channels = match info.pixel_format {
        jpeg_decoder::PixelFormat::L8 => 1,
        jpeg_decoder::PixelFormat::RGB24 => 3,
        _ => return Err(invalid("image color format is unsupported")),
    };
    let decoded = decoder
        .decode()
        .map_err(|_| invalid("image is malformed"))?;
    Ok(to_rgba(width, height, &decoded, channels))
}

fn to_rgba(width: u32, height: u32, decoded: &[u8], channels: usize) -> Raster {
    let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
    for source in decoded.chunks_exact(channels) {
        match channels {
            1 => pixels.extend_from_slice(&[source[0], source[0], source[0], 255]),
            2 => pixels.extend_from_slice(&[source[0], source[0], source[0], source[1]]),
            3 => pixels.extend_from_slice(&[source[0], source[1], source[2], 255]),
            _ => pixels.extend_from_slice(source),
        }
    }
    Raster {
        width,
        height,
        pixels,
    }
}

/// Area-average downscale, so text and thin UI lines stay legible.
fn downscale(raster: Raster, edge: u32) -> Raster {
    let long = raster.width.max(raster.height);
    if long <= edge {
        return raster;
    }
    let scale =
        |value: u32| ((u64::from(value) * u64::from(edge)).div_ceil(u64::from(long))) as u32;
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
                    for (total, channel) in sum.iter_mut().zip(pixel) {
                        *total += u64::from(*channel);
                    }
                }
            }
            let count = ((bottom - top) * (right - left)) as u64;
            let target = (y * width as usize + x) * 4;
            for (channel, total) in sum.iter().enumerate() {
                pixels[target + channel] = ((total + count / 2) / count) as u8;
            }
        }
    }
    Raster {
        width,
        height,
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
    Ok(output)
}

/// JPEG has no alpha; transparent areas are composed onto white.
fn encode_jpeg(raster: &Raster) -> AppResult<Vec<u8>> {
    let rgb = raster
        .pixels
        .chunks_exact(4)
        .flat_map(|pixel| {
            let alpha = u16::from(pixel[3]);
            let blend =
                |channel: u8| ((u16::from(channel) * alpha + 255 * (255 - alpha)) / 255) as u8;
            [blend(pixel[0]), blend(pixel[1]), blend(pixel[2])]
        })
        .collect::<Vec<_>>();
    let (width, height) = (
        u16::try_from(raster.width).map_err(|_| invalid("image is too wide"))?,
        u16::try_from(raster.height).map_err(|_| invalid("image is too tall"))?,
    );
    let mut output = Vec::new();
    jpeg_encoder::Encoder::new(&mut output, JPEG_QUALITY)
        .encode(&rgb, width, height, jpeg_encoder::ColorType::Rgb)
        .map_err(|_| invalid("image could not be re-encoded"))?;
    Ok(output)
}
