use super::*;

fn png_of(
    width: u32,
    height: u32,
    color: png::ColorType,
    depth: png::BitDepth,
    data: &[u8],
) -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(color);
        encoder.set_depth(depth);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(data)
            .unwrap();
    }
    bytes
}

/// A photo-like RGB raster: smooth gradients under fine grain.
fn photo(width: u32, height: u32) -> Vec<u8> {
    let mut state = 0x9e37_79b9_u32;
    let mut pixels = Vec::with_capacity(width as usize * height as usize * 3);
    for y in 0..height {
        for x in 0..width {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let grain = (state % 6) as u8;
            pixels.extend_from_slice(&[
                ((x * 200 / width) as u8).saturating_add(grain),
                ((y * 200 / height) as u8).saturating_add(grain),
                (((x + y) * 100 / (width + height)) as u8).saturating_add(grain),
            ]);
        }
    }
    pixels
}

fn decoded(image: &Normalized) -> Raster {
    decode(image.media_type, &image.bytes).unwrap()
}

fn pixel(raster: &Raster, x: u32, y: u32) -> [u8; 4] {
    let at = ((y * raster.width + x) * 4) as usize;
    raster.pixels[at..at + 4].try_into().unwrap()
}

fn tiff_orientation(value: u8) -> Vec<u8> {
    let mut tiff = b"II*\0".to_vec();
    tiff.extend_from_slice(&8_u32.to_le_bytes());
    tiff.extend_from_slice(&1_u16.to_le_bytes());
    tiff.extend_from_slice(&0x0112_u16.to_le_bytes());
    tiff.extend_from_slice(&3_u16.to_le_bytes());
    tiff.extend_from_slice(&1_u32.to_le_bytes());
    tiff.extend_from_slice(&[value, 0, 0, 0]);
    tiff.extend_from_slice(&0_u32.to_le_bytes());
    tiff
}

/// A JPEG whose stored pixels are red on the left and blue on the right,
/// tagged with an EXIF orientation.
fn oriented_jpeg(width: u16, height: u16, orientation: u8) -> Vec<u8> {
    let rgb = (0..height)
        .flat_map(|_| {
            (0..width).map(move |x| {
                if x < width / 2 {
                    [220, 20, 20]
                } else {
                    [20, 20, 220]
                }
            })
        })
        .flatten()
        .collect::<Vec<_>>();
    let mut bytes = Vec::new();
    let mut encoder = jpeg_encoder::Encoder::new(&mut bytes, 90);
    encoder
        .add_exif_metadata(&tiff_orientation(orientation))
        .unwrap();
    encoder
        .encode(&rgb, width, height, jpeg_encoder::ColorType::Rgb)
        .unwrap();
    bytes
}

#[test]
fn an_exif_rotated_photo_is_stored_and_sent_upright() {
    let jpeg = oriented_jpeg(3000, 1000, 6);
    assert_eq!(header(ImageMediaType::Jpeg, &jpeg).unwrap().orientation, 6);

    for bounds in [STORED, PROMPT] {
        let image = normalize(ImageMediaType::Jpeg, &jpeg, bounds).unwrap();
        let raster = decoded(&image);
        let long = 3000.min(bounds.edge);
        assert_eq!(
            (raster.width, raster.height),
            ((1000 * long).div_ceil(3000), long)
        );
        // Rotated 90 degrees clockwise: the stored left half is now on top.
        let top = pixel(&raster, raster.width / 2, raster.height / 4);
        let bottom = pixel(&raster, raster.width / 2, raster.height * 3 / 4);
        assert!(top[0] > 150 && top[2] < 100, "{top:?}");
        assert!(bottom[2] > 150 && bottom[0] < 100, "{bottom:?}");
    }

    // Within the stored bounds it is kept byte for byte, EXIF and all, but a
    // prompt re-encodes it upright for a reader that may ignore EXIF.
    let small = oriented_jpeg(1200, 400, 6);
    assert_eq!(
        normalize(ImageMediaType::Jpeg, &small, STORED)
            .unwrap()
            .bytes,
        small
    );
    let sent = decoded(&normalize(ImageMediaType::Jpeg, &small, PROMPT).unwrap());
    assert_eq!((sent.width, sent.height), (400, 1200));
}

#[test]
fn every_exif_orientation_maps_the_stored_corner_to_its_upright_place() {
    // A 3x2 raster whose top-left stored pixel is marked.
    let raster = || {
        let mut pixels = vec![0_u8; 3 * 2 * 4];
        pixels[..4].copy_from_slice(&[255, 255, 255, 255]);
        Raster {
            width: 3,
            height: 2,
            pixels,
        }
    };
    let corner = |orientation: u8| {
        let upright = orient(raster(), orientation);
        let marked = upright
            .pixels
            .chunks_exact(4)
            .position(|pixel| pixel[0] == 255)
            .unwrap();
        (
            upright.width,
            upright.height,
            marked as u32 % upright.width,
            marked as u32 / upright.width,
        )
    };
    assert_eq!(corner(1), (3, 2, 0, 0));
    assert_eq!(corner(2), (3, 2, 2, 0));
    assert_eq!(corner(3), (3, 2, 2, 1));
    assert_eq!(corner(4), (3, 2, 0, 1));
    assert_eq!(corner(5), (2, 3, 0, 0));
    assert_eq!(corner(6), (2, 3, 1, 0));
    assert_eq!(corner(7), (2, 3, 1, 2));
    assert_eq!(corner(8), (2, 3, 0, 2));
}

#[test]
fn a_photo_is_sent_as_the_smaller_jpeg_and_a_screenshot_stays_png() {
    let photo = png_of(
        3000,
        2000,
        png::ColorType::Rgb,
        png::BitDepth::Eight,
        &photo(3000, 2000),
    );
    let image = normalize(ImageMediaType::Png, &photo, PROMPT).unwrap();
    assert_eq!(image.media_type, ImageMediaType::Jpeg);
    assert!(image.bytes.len() <= PROMPT_IMAGE_TOTAL_BYTES);
    let raster = decoded(&image);
    assert_eq!((raster.width, raster.height), (1568, 1046));
    let scaled = downscale(decode(ImageMediaType::Png, &photo).unwrap(), PROMPT.edge);
    let png = encode_png(&scaled, true).unwrap();
    assert!(png.len() > PROMPT_IMAGE_PNG_BYTES && image.bytes.len() < png.len());

    let screenshot = crate::image_artifacts::screenshot_png(3840, 2160);
    let image = normalize(ImageMediaType::Png, &screenshot, PROMPT).unwrap();
    assert_eq!(image.media_type, ImageMediaType::Png);
    assert!(image.bytes.len() <= PROMPT_IMAGE_PNG_BYTES);
}

#[test]
fn past_the_png_size_the_smaller_encoding_wins_even_when_it_is_png() {
    // Flat colour compresses better as PNG than as JPEG.
    let flat = png_of(
        2000,
        2000,
        png::ColorType::Rgb,
        png::BitDepth::Eight,
        &[90; 2000 * 2000 * 3],
    );
    let bounds = Bounds {
        png_bytes: 0,
        ..PROMPT
    };
    let image = normalize(ImageMediaType::Png, &flat, bounds).unwrap();
    let scaled = downscale(decode(ImageMediaType::Png, &flat).unwrap(), PROMPT.edge);
    let (png, jpeg) = (
        encode_png(&scaled, true).unwrap(),
        encode_jpeg(&scaled).unwrap(),
    );
    assert_eq!(image.bytes, if jpeg.len() < png.len() { jpeg } else { png });
}

#[test]
fn transparency_is_composited_onto_white_for_jpeg() {
    let raster = Raster {
        width: 16,
        height: 16,
        pixels: [[200, 0, 0, 0], [0, 0, 0, 128]]
            .iter()
            .flat_map(|pixel| std::iter::repeat_n(*pixel, 128))
            .flatten()
            .collect(),
    };
    let jpeg = encode_jpeg(&raster).unwrap();
    let image = decoded(&Normalized {
        media_type: ImageMediaType::Jpeg,
        bytes: jpeg,
    });
    let clear = pixel(&image, 8, 2);
    let half = pixel(&image, 8, 13);
    assert!(
        clear[..3].iter().all(|channel| *channel >= 245),
        "{clear:?}"
    );
    assert!(
        half[..3]
            .iter()
            .all(|channel| (118..=138).contains(channel)),
        "{half:?}"
    );
}

#[test]
fn downscaling_weights_colour_by_alpha() {
    let raster = Raster {
        width: 4,
        height: 1,
        pixels: [
            [255, 0, 0, 255],
            [0, 255, 0, 0],
            [255, 0, 0, 255],
            [0, 255, 0, 0],
        ]
        .concat(),
    };
    let scaled = downscale(raster, 2);
    assert_eq!(scaled.pixels, [[255, 0, 0, 128], [255, 0, 0, 128]].concat());
}

#[test]
fn sixteen_bit_and_greyscale_pngs_are_normalised() {
    let wide: Vec<u8> = (0..3000_u32 * 10)
        .flat_map(|index| {
            let value = (index % 3000 * 65_535 / 2999) as u16;
            [
                value.to_be_bytes(),
                0_u16.to_be_bytes(),
                0xffff_u16.to_be_bytes(),
            ]
            .concat()
        })
        .collect();
    let sixteen = png_of(3000, 10, png::ColorType::Rgb, png::BitDepth::Sixteen, &wide);
    let grey_data: Vec<u8> = (0..3000_u32 * 10)
        .map(|index| (index % 3000 * 255 / 2999) as u8)
        .collect();
    let grey = png_of(
        3000,
        10,
        png::ColorType::Grayscale,
        png::BitDepth::Eight,
        &grey_data,
    );

    for source in [sixteen, grey] {
        let image = normalize(ImageMediaType::Png, &source, PROMPT).unwrap();
        assert_eq!(image.media_type, ImageMediaType::Png);
        let raster = decoded(&image);
        assert_eq!((raster.width, raster.height), (1568, 6));
        assert!(pixel(&raster, 0, 0)[0] < 8);
        assert!(pixel(&raster, 1567, 5)[0] > 247);
        assert_eq!(pixel(&raster, 700, 3)[3], 255);
    }
}

#[test]
fn an_animated_png_past_the_bounds_is_refused_as_animated() {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 3000, 10);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_animated(1, 0).unwrap();
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&vec![0; 3000 * 10 * 3])
            .unwrap();
    }
    let error = normalize(ImageMediaType::Png, &bytes, PROMPT).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("animated PNG images are unsupported"),
        "{error}"
    );
}

#[test]
fn an_oversized_header_is_refused_with_its_limits_before_any_decode() {
    // Only a signature and IHDR: there is nothing to decode.
    let header_only = |width: u32, height: u32| {
        let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes.extend_from_slice(&[8, 2, 0, 0, 0, 0, 0, 0, 0]);
        bytes
    };
    for (width, height) in [(9000, 10), (5000, 5000)] {
        let error =
            normalize(ImageMediaType::Png, &header_only(width, height), PROMPT).unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains(&format!("{width}x{height} px")),
            "{message}"
        );
        assert!(
            message.contains("8192 px on a side and 16 megapixels"),
            "{message}"
        );
    }
    let error = normalize(ImageMediaType::Png, &header_only(100, 100)[..20], PROMPT).unwrap_err();
    assert!(
        error.to_string().contains("corrupt or truncated"),
        "{error}"
    );
}

#[test]
fn a_large_photo_for_any_harness_is_stored_at_the_store_bounds() {
    let photo = png_of(
        3000,
        2000,
        png::ColorType::Rgb,
        png::BitDepth::Eight,
        &photo(3000, 2000),
    );
    let image = normalize(ImageMediaType::Png, &photo, STORED).unwrap();
    let raster = decoded(&image);
    assert_eq!((raster.width, raster.height), (2048, 1366));
    assert!(image.bytes.len() <= STORED_IMAGE_BYTES);
}
