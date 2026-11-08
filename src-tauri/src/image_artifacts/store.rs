use std::{fs, io::Cursor, path::Path};

use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    acp::{intent::ImageMediaType, limits::IMAGE_PREVIEW_BYTES, migration},
    error::{AppError, AppResult},
    image_attachment::{ImageAttachedContext, ImageAttachmentKind, MAX_OUTGOING_IMAGE_BYTES},
};
const FULL_LIMIT: u64 = 4 * 1024 * 1024;

pub(crate) fn ingest_image(
    connection: &Connection,
    root: &Path,
    project_id: &str,
    chat_id: &str,
    media_type: ImageMediaType,
    bytes: &[u8],
    label: &str,
) -> AppResult<ImageAttachedContext> {
    if bytes.is_empty() || bytes.len() > MAX_OUTGOING_IMAGE_BYTES as usize {
        return Err(invalid("image exceeds the 4 MiB bound"));
    }
    if label.is_empty()
        || label.len() > 80
        || label
            .chars()
            .any(crate::acp::attached_context::has_unsafe_display_character)
    {
        return Err(invalid("image label is invalid"));
    }
    if !matches!(media_type, ImageMediaType::Png | ImageMediaType::Jpeg)
        || (media_type == ImageMediaType::Png && !bytes.starts_with(b"\x89PNG\r\n\x1a\n"))
        || (media_type == ImageMediaType::Jpeg
            && (!bytes.starts_with(b"\xff\xd8") || !bytes.ends_with(b"\xff\xd9")))
    {
        return Err(invalid("outgoing image type is unsupported or mismatched"));
    }
    let (width, height, thumbnail) = match media_type {
        ImageMediaType::Png => admit_png(bytes)?,
        ImageMediaType::Jpeg => admit_jpeg(bytes)?,
        ImageMediaType::Webp => unreachable!(),
    };
    validate_root(root)?;
    let id = Uuid::new_v4().to_string();
    let hash = format!("{:x}", Sha256::digest(bytes));
    let stage = root.join(format!("{id}.full.{}.stage", Uuid::new_v4()));
    let final_path = full_path(root, &id);
    let thumb_stage = root.join(format!("{id}.thumb.{}.stage", Uuid::new_v4()));
    let thumb_final = thumb_path(root, &id);
    let stored = (|| -> AppResult<()> {
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&stage)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        let mut thumb = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&thumb_stage)?;
        thumb.write_all(&thumbnail)?;
        thumb.sync_all()?;
        fs::rename(&stage, &final_path)?;
        fs::rename(&thumb_stage, &thumb_final)?;
        connection.execute(
            "INSERT INTO chat_image_artifacts(
             id,project_id,chat_id,queue_id,session_id,turn_id,lifecycle,mime_type,
             width,height,full_bytes,full_sha256,thumbnail_bytes,thumbnail_sha256,
             label,created_at,updated_at
             ) VALUES (?1,?2,?3,NULL,NULL,NULL,'pending',?4,?5,?6,?7,?8,?9,?10,?11,unixepoch(),unixepoch())",
            params![id, project_id, chat_id, media_type.as_str(), width, height, bytes.len() as i64, hash, thumbnail.len() as i64, format!("{:x}", Sha256::digest(&thumbnail)), label],
        )?;
        Ok(())
    })();
    if stored.is_err() {
        let _ = fs::remove_file(&stage);
        let _ = fs::remove_file(&thumb_stage);
        let _ = fs::remove_file(&final_path);
        let _ = fs::remove_file(&thumb_final);
    }
    stored?;
    Ok(ImageAttachedContext {
        kind: ImageAttachmentKind::Image,
        artifact_id: id,
        media_type,
        width,
        height,
        byte_length: bytes.len() as u32,
        sha256: hash,
        label: label.into(),
    })
}

fn admit_png(bytes: &[u8]) -> AppResult<(u32, u32, Vec<u8>)> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_ignore_text_chunk(true);
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder
        .read_info()
        .map_err(|_| invalid("image is malformed"))?;
    let info = reader.info();
    let (width, height) = (info.width, info.height);
    if !valid_dimensions(width, height) || info.animation_control.is_some() {
        return Err(invalid("image dimensions or animation are unsupported"));
    }
    let mut decoded = vec![0; reader.output_buffer_size()];
    let frame = reader
        .next_frame(&mut decoded)
        .map_err(|_| invalid("image is malformed"))?;
    let thumbnail = if bytes.len() <= IMAGE_PREVIEW_BYTES {
        bytes.to_vec()
    } else {
        let thumb_width = width.min(128);
        let thumb_height = height.min(128);
        let mut pixels = vec![0; thumb_width as usize * thumb_height as usize * 4];
        let stride = match frame.color_type {
            png::ColorType::Grayscale => 1,
            png::ColorType::GrayscaleAlpha => 2,
            png::ColorType::Rgb => 3,
            png::ColorType::Rgba => 4,
            png::ColorType::Indexed => return Err(invalid("image color format is unsupported")),
        };
        for y in 0..thumb_height as usize {
            for x in 0..thumb_width as usize {
                let source_x = x * width as usize / thumb_width as usize;
                let source_y = y * height as usize / thumb_height as usize;
                let source = (source_y * width as usize + source_x) * stride;
                let target = (y * thumb_width as usize + x) * 4;
                match frame.color_type {
                    png::ColorType::Grayscale => {
                        pixels[target..target + 3].fill(decoded[source]);
                        pixels[target + 3] = 255;
                    }
                    png::ColorType::GrayscaleAlpha => {
                        pixels[target..target + 3].fill(decoded[source]);
                        pixels[target + 3] = decoded[source + 1];
                    }
                    png::ColorType::Rgb => {
                        pixels[target..target + 3].copy_from_slice(&decoded[source..source + 3]);
                        pixels[target + 3] = 255;
                    }
                    png::ColorType::Rgba => {
                        pixels[target..target + 4].copy_from_slice(&decoded[source..source + 4]);
                    }
                    png::ColorType::Indexed => unreachable!(),
                }
            }
        }
        let mut thumbnail = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut thumbnail, thumb_width, thumb_height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .and_then(|mut writer| writer.write_image_data(&pixels))
                .map_err(|_| invalid("image preview could not be encoded"))?;
        }
        if thumbnail.len() > 262_144 {
            return Err(invalid("image preview exceeds its bound"));
        }
        thumbnail
    };
    Ok((width, height, thumbnail))
}

fn admit_jpeg(bytes: &[u8]) -> AppResult<(u32, u32, Vec<u8>)> {
    let mut decoder = jpeg_decoder::Decoder::new(Cursor::new(bytes));
    decoder
        .read_info()
        .map_err(|_| invalid("image is malformed"))?;
    let info = decoder
        .info()
        .ok_or_else(|| invalid("image is malformed"))?;
    let (width, height) = (u32::from(info.width), u32::from(info.height));
    if !valid_dimensions(width, height) {
        return Err(invalid("image dimensions are unsupported"));
    }
    let decoded = decoder
        .decode()
        .map_err(|_| invalid("image is malformed"))?;
    let thumbnail = if bytes.len() <= IMAGE_PREVIEW_BYTES {
        bytes.to_vec()
    } else {
        let stride = match info.pixel_format {
            jpeg_decoder::PixelFormat::L8 => 1,
            jpeg_decoder::PixelFormat::RGB24 => 3,
            _ => return Err(invalid("image color format is unsupported")),
        };
        let thumb_width = width.min(128);
        let thumb_height = height.min(128);
        let mut pixels = vec![0; thumb_width as usize * thumb_height as usize * 4];
        for y in 0..thumb_height as usize {
            for x in 0..thumb_width as usize {
                let source_x = x * width as usize / thumb_width as usize;
                let source_y = y * height as usize / thumb_height as usize;
                let source = (source_y * width as usize + source_x) * stride;
                let target = (y * thumb_width as usize + x) * 4;
                if stride == 1 {
                    pixels[target..target + 3].fill(decoded[source]);
                } else {
                    pixels[target..target + 3].copy_from_slice(&decoded[source..source + 3]);
                }
                pixels[target + 3] = 255;
            }
        }
        let mut thumbnail = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut thumbnail, thumb_width, thumb_height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .and_then(|mut writer| writer.write_image_data(&pixels))
                .map_err(|_| invalid("image preview could not be encoded"))?;
        }
        if thumbnail.len() > 262_144 {
            return Err(invalid("image preview exceeds its bound"));
        }
        thumbnail
    };
    Ok((width, height, thumbnail))
}

fn valid_dimensions(width: u32, height: u32) -> bool {
    width > 0
        && height > 0
        && width <= 2048
        && height <= 2048
        && u64::from(width) * u64::from(height) <= 4_194_304
}

fn invalid(message: &str) -> AppError {
    AppError::InvalidRequest(message.into())
}

pub(super) fn validate_root(root: &Path) -> AppResult<()> {
    if !root.is_absolute()
        || root.file_name().and_then(|name| name.to_str()) != Some("image-artifacts")
    {
        return Err(invalid("image artifact root is not canonical"));
    }
    fs::create_dir_all(root)?;
    let metadata = fs::symlink_metadata(root)?;
    #[cfg(unix)]
    let aliased = metadata.file_type().is_symlink();
    #[cfg(windows)]
    let aliased = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x0000_0400 != 0
    };
    if aliased || !metadata.is_dir() {
        return Err(invalid(
            "image artifact root is an alias or not a directory",
        ));
    }
    Ok(())
}

pub(super) fn full_path(root: &Path, id: &str) -> std::path::PathBuf {
    root.join(format!("{id}.full"))
}

pub(super) fn thumb_path(root: &Path, id: &str) -> std::path::PathBuf {
    root.join(format!("{id}.thumb"))
}

pub(crate) fn remove_pending_image(
    connection: &Connection,
    root: &Path,
    project_id: &str,
    chat_id: &str,
    artifact_id: &str,
) -> AppResult<()> {
    let id = Uuid::parse_str(artifact_id).map_err(|_| invalid("image artifact id is invalid"))?;
    if id.to_string() != artifact_id || id.get_version() != Some(uuid::Version::Random) {
        return Err(invalid("image artifact id is not canonical"));
    }
    validate_root(root)?;
    let removed = connection.execute(
        "DELETE FROM chat_image_artifacts
         WHERE id=?1 AND project_id=?2 AND chat_id=?3 AND lifecycle='pending'",
        params![artifact_id, project_id, chat_id],
    )?;
    if removed != 1 {
        return Err(invalid("pending image artifact is unavailable"));
    }
    migration::remove_image_file(&full_path(root, artifact_id))?;
    migration::remove_image_file(&thumb_path(root, artifact_id))?;
    Ok(())
}

pub(crate) fn verify_image_reference(
    connection: &Connection,
    root: &Path,
    project_id: &str,
    chat_id: &str,
    image: &ImageAttachedContext,
) -> AppResult<Vec<u8>> {
    let id =
        Uuid::parse_str(&image.artifact_id).map_err(|_| invalid("image artifact id is invalid"))?;
    if id.to_string() != image.artifact_id {
        return Err(invalid("image artifact id is not canonical"));
    }
    validate_root(root)?;
    let row: Option<(String, i64, i64, i64, String, String, String)> = connection
        .query_row(
            "SELECT mime_type,width,height,full_bytes,full_sha256,lifecycle,label
             FROM chat_image_artifacts WHERE id=?1 AND project_id=?2 AND chat_id=?3",
            params![image.artifact_id, project_id, chat_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            },
        )
        .optional()?;
    let Some((mime, width, height, length, hash, state, label)) = row else {
        return Err(invalid("image artifact is unavailable"));
    };
    let expected_mime = match image.media_type {
        ImageMediaType::Png => "image/png",
        ImageMediaType::Jpeg => "image/jpeg",
        ImageMediaType::Webp => return Err(invalid("outgoing WebP is unsupported")),
    };
    if mime != expected_mime
        || width != i64::from(image.width)
        || height != i64::from(image.height)
        || length != i64::from(image.byte_length)
        || hash != image.sha256
        || label != image.label
        || !matches!(state.as_str(), "pending" | "queued" | "inFlight")
    {
        return Err(invalid(
            "image artifact reference does not match its owner-bound record",
        ));
    }
    let bytes = migration::read_image_file(&full_path(root, &image.artifact_id), FULL_LIMIT)?;
    if bytes.len() != length as usize || format!("{:x}", Sha256::digest(&bytes)) != hash {
        return Err(invalid("image artifact is missing or corrupt"));
    }
    Ok(bytes)
}
