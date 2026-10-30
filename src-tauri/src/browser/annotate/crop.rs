use super::{
    area::AREA_MASKS,
    element::{ElementBox, Rect, Viewport},
    selection::Selection,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Serialize;
use std::io::Cursor;

const PIXELS: u64 = 16_777_216;
const CROP_SIDE: u32 = 1024;
const CROP_BYTES: usize = 4 * 1024 * 1024;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CropPreview {
    width: u32,
    height: u32,
    png_base64: String,
}

pub(super) fn preview(selection: &Selection) -> Result<CropPreview, String> {
    if !selection.crop_coverage {
        return Err("Picture unavailable: complete form-control masking could not be established for this page".into());
    }
    let frame = &selection.frame;
    let data = frame
        .png_base64
        .as_ref()
        .ok_or("lossless browser crop unavailable")?;
    if data.len() > 8_000_000 || u64::from(frame.width) * u64::from(frame.height) > PIXELS {
        return Err("browser crop frame exceeds limit".into());
    }
    let bytes = STANDARD
        .decode(data)
        .map_err(|_| "browser crop unavailable")?;
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_limits(png::Limits {
        bytes: (PIXELS * 4) as usize,
    });
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder
        .read_info()
        .map_err(|_| "browser crop unavailable")?;
    if reader.info().width != frame.width || reader.info().height != frame.height {
        return Err("browser crop frame rejected".into());
    }
    let mut decoded = vec![0; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut decoded)
        .map_err(|_| "browser crop unavailable")?;
    decoded.truncate(info.buffer_size());
    let rgb = match info.color_type {
        png::ColorType::Rgb => decoded,
        png::ColorType::Rgba => decoded
            .chunks_exact(4)
            .flat_map(|pixel| [pixel[0], pixel[1], pixel[2]])
            .collect(),
        png::ColorType::Grayscale => decoded.into_iter().flat_map(|value| [value; 3]).collect(),
        _ => return Err("browser crop format unavailable".into()),
    };
    let (width, height, pixels) = crop_rgb(
        &rgb,
        frame.width,
        frame.height,
        selection.viewport,
        selection.picked.bounds,
        &selection.masks,
    )?;
    let png = encode(width, height, &pixels)?;
    Ok(CropPreview {
        width,
        height,
        png_base64: STANDARD.encode(png),
    })
}

fn encode(width: u32, height: u32, pixels: &[u8]) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|_| "browser crop unavailable")?;
        writer
            .write_image_data(pixels)
            .map_err(|_| "browser crop unavailable")?;
    }
    if bytes.len() > CROP_BYTES {
        return Err("browser crop exceeds limit".into());
    }
    Ok(bytes)
}

fn crop_rgb(
    pixels: &[u8],
    width: u32,
    height: u32,
    viewport: Viewport,
    bounds: ElementBox,
    masks: &[Rect],
) -> Result<(u32, u32, Vec<u8>), String> {
    let count = u64::from(width) * u64::from(height);
    if width == 0
        || height == 0
        || count > PIXELS
        || pixels.len() as u64 != count * 3
        || !viewport.admitted()
        || masks.len() > AREA_MASKS
        || bounds.width < 1
        || bounds.height < 1
        || (bounds.x as f64) < viewport.x
        || (bounds.y as f64) < viewport.y
        || (bounds.x as f64 + bounds.width as f64) > viewport.x + viewport.client_width + 1.0
        || (bounds.y as f64 + bounds.height as f64) > viewport.y + viewport.client_height + 1.0
        || masks.iter().any(|rect| {
            ![rect.x, rect.y, rect.width, rect.height]
                .iter()
                .all(|value| value.is_finite() && value.abs() <= 10_000_000.0)
                || rect.width < 0.0
                || rect.height < 0.0
        })
    {
        return Err("browser crop bounds rejected".into());
    }
    let scale = (f64::from(width) / viewport.width).min(f64::from(height) / viewport.height);
    let native_width = (bounds.width as f64 * scale).ceil().max(1.0);
    let native_height = (bounds.height as f64 * scale).ceil().max(1.0);
    let downscale = (f64::from(CROP_SIDE) / native_width.max(native_height)).min(1.0);
    let output_width = (native_width * downscale).floor().max(1.0) as u32;
    let output_height = (native_height * downscale).floor().max(1.0) as u32;
    let mut output = vec![0; (output_width * output_height * 3) as usize];
    for row in 0..output_height {
        for column in 0..output_width {
            let page_x =
                bounds.x as f64 + f64::from(column) * bounds.width as f64 / f64::from(output_width);
            let page_y =
                bounds.y as f64 + f64::from(row) * bounds.height as f64 / f64::from(output_height);
            let source_x = ((page_x - viewport.x) * f64::from(width) / viewport.width)
                .floor()
                .clamp(0.0, f64::from(width - 1)) as u32;
            let source_y = ((page_y - viewport.y) * f64::from(height) / viewport.height)
                .floor()
                .clamp(0.0, f64::from(height - 1)) as u32;
            let source = ((source_y * width + source_x) * 3) as usize;
            let target = ((row * output_width + column) * 3) as usize;
            output[target..target + 3].copy_from_slice(&pixels[source..source + 3]);
        }
        let top =
            bounds.y as f64 + f64::from(row) * bounds.height as f64 / f64::from(output_height);
        let bottom = top + bounds.height as f64 / f64::from(output_height);
        let output_x = |page_x: f64| {
            ((page_x - bounds.x as f64) * f64::from(output_width) / bounds.width as f64)
                .clamp(0.0, f64::from(output_width))
        };
        let mut intervals: Vec<(usize, usize)> = masks
            .iter()
            .filter(|mask| bottom > mask.y - 1.0 && top < mask.y + mask.height + 1.0)
            .map(|mask| {
                (
                    output_x(mask.x - 1.0).floor() as usize,
                    output_x(mask.x + mask.width + 1.0).ceil() as usize,
                )
            })
            .collect();
        intervals.sort_unstable();
        let mut end = 0;
        for (from, to) in intervals {
            let from = from.max(end);
            if from < to {
                let row_start = (row * output_width * 3) as usize;
                output[row_start + from * 3..row_start + to * 3].fill(0);
                end = to;
            }
        }
    }
    Ok((output_width, output_height, output))
}

#[cfg(test)]
#[path = "crop_tests.rs"]
mod tests;
