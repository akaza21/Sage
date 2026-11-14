use std::{fs, path::Path};

use rusqlite::{Connection, params};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    acp::intent::ImageMediaType,
    image_attachment::{ImageAttachedContext, ImageAttachmentKind},
};

use super::store::{full_path, thumb_path, validate_root};

/// A real 1x1 grey-and-alpha PNG, so a send can decode it.
const PNG: &[u8] = b"\x89\x50\x4e\x47\x0d\x0a\x1a\x0a\x00\x00\x00\x0d\x49\x48\x44\x52\x00\x00\x00\x01\x00\x00\x00\x01\x08\x04\x00\x00\x00\xb5\x1c\x0c\x02\x00\x00\x00\x0b\x49\x44\x41\x54\x78\xda\x63\xfc\xff\x1f\x00\x03\x03\x02\x00\xef\xa2\xa7\x5b\x00\x00\x00\x00\x49\x45\x4e\x44\xae\x42\x60\x82";

pub(crate) fn insert_test_image(
    connection: &Connection,
    root: &Path,
    project_id: &str,
    chat_id: &str,
    label: &str,
) -> ImageAttachedContext {
    validate_root(root).unwrap();
    let id = Uuid::new_v4().to_string();
    fs::write(full_path(root, &id), PNG).unwrap();
    fs::write(thumb_path(root, &id), PNG).unwrap();
    let hash = format!("{:x}", Sha256::digest(PNG));
    connection
        .execute(
            "INSERT INTO chat_image_artifacts(
           id,project_id,chat_id,queue_id,session_id,turn_id,lifecycle,mime_type,
           width,height,full_bytes,full_sha256,thumbnail_bytes,thumbnail_sha256,
           label,created_at,updated_at
         ) VALUES (?1,?2,?3,NULL,NULL,NULL,'pending','image/png',1,1,?4,?5,?4,?5,?6,1,1)",
            params![id, project_id, chat_id, PNG.len() as i64, hash, label],
        )
        .unwrap();
    ImageAttachedContext {
        kind: ImageAttachmentKind::Image,
        artifact_id: id,
        media_type: ImageMediaType::Png,
        width: 1,
        height: 1,
        byte_length: PNG.len() as u32,
        sha256: hash,
        label: label.into(),
    }
}

/// Stores `bytes` through the real upload path, normalising it when needed.
pub(crate) fn ingest_test_image(
    connection: &Connection,
    root: &Path,
    project_id: &str,
    chat_id: &str,
    bytes: &[u8],
) -> sage_core::error::AppResult<ImageAttachedContext> {
    super::store::ingest_image(
        connection,
        root,
        project_id,
        chat_id,
        ImageMediaType::Png,
        bytes,
        "screenshot.png",
    )
}

fn encode_rgb(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(pixels)
            .unwrap();
    }
    bytes
}

/// A UI screenshot: flat panels, rules and dense rows of glyph-like marks.
pub(crate) fn screenshot_png(width: u32, height: u32) -> Vec<u8> {
    let mut pixels = Vec::with_capacity(width as usize * height as usize * 3);
    for y in 0..height {
        for x in 0..width {
            let panel = if x < width / 5 {
                [32, 34, 40]
            } else {
                [250, 250, 252]
            };
            let rule = y % 120 == 0 || x == width / 5;
            let glyph = (y % 40) < 14 && (x % 9) < 6 && ((x / 9 + y / 40) % 7) != 0;
            let pixel = if rule {
                [200, 200, 210]
            } else if glyph {
                [panel[0] ^ 0xc0, panel[1] ^ 0xc0, panel[2] ^ 0xc0]
            } else {
                panel
            };
            pixels.extend_from_slice(&pixel);
        }
    }
    encode_rgb(width, height, &pixels)
}

/// Incompressible RGB noise as a PNG.
pub(crate) fn noise_png(width: u32, height: u32) -> Vec<u8> {
    let mut state = 0x2545_f491_u32;
    let pixels = (0..width as usize * height as usize * 3)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as u8
        })
        .collect::<Vec<_>>();
    encode_rgb(width, height, &pixels)
}

pub(crate) fn png_dimensions(bytes: &[u8]) -> (u32, u32) {
    let reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .unwrap();
    (reader.info().width, reader.info().height)
}
