//! The square chip thumbnail the store keeps for each attached image.
use sage_acp_protocol::intent::ImageMediaType;
use sage_core::error::AppResult;

use super::normalize::{Raster, decode, downscale, encode_png, header, orient};

/// A square, upright PNG of the image's centre, `edge` px on a side (or the
/// short side, if smaller), for an attachment chip.
pub(crate) fn square_thumbnail(
    media_type: ImageMediaType,
    bytes: &[u8],
    edge: u32,
) -> AppResult<Vec<u8>> {
    let orientation = header(media_type, bytes)?.orientation;
    // The centre square is the same pixels in either orientation, so it is
    // cropped and shrunk before the (smaller) result is turned upright.
    let raster = orient(
        downscale(crop_square(decode(media_type, bytes)?), edge),
        orientation,
    );
    let opaque = raster.pixels.chunks_exact(4).all(|pixel| pixel[3] == 255);
    encode_png(&raster, opaque)
}

/// The centre square of `raster`, compacted in place.
fn crop_square(mut raster: Raster) -> Raster {
    let side = raster.width.min(raster.height) as usize;
    let (width, height) = (raster.width as usize, raster.height as usize);
    let (left, top) = ((width - side) / 2, (height - side) / 2);
    for row in 0..side {
        let start = ((top + row) * width + left) * 4;
        raster
            .pixels
            .copy_within(start..start + side * 4, row * side * 4);
    }
    raster.pixels.truncate(side * side * 4);
    Raster {
        width: side as u32,
        height: side as u32,
        pixels: raster.pixels,
    }
}
