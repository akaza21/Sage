use super::*;

fn viewport() -> Viewport {
    Viewport {
        x: 0.0,
        y: 0.0,
        width: 8.0,
        height: 8.0,
        client_width: 8.0,
        client_height: 8.0,
        scale: 1.0,
    }
}

#[test]
fn crop_refuses_incomplete_control_mask_coverage_before_decoding() {
    let mut selection = crate::browser::annotate::selection::tests::selection("browser-1");
    selection.crop_coverage = false;
    assert!(preview(&selection).err().unwrap().contains("masking"));
}

#[test]
fn crop_requires_lossless_pixels_and_masks_them_before_preview_encoding() {
    let mut selection = crate::browser::annotate::selection::tests::selection("browser-1");
    assert!(preview(&selection).err().unwrap().contains("lossless"));
    selection.viewport = viewport();
    selection.picked.bounds = ElementBox {
        x: 0,
        y: 0,
        width: 8,
        height: 8,
    };
    selection.frame.width = 8;
    selection.frame.height = 8;
    selection.frame.png_base64 = Some(STANDARD.encode(encode(8, 8, &[255; 8 * 8 * 3]).unwrap()));
    selection.masks = vec![Rect {
        x: 4.0,
        y: 4.0,
        width: 2.0,
        height: 2.0,
    }];
    let crop = preview(&selection).unwrap();
    assert_eq!((crop.width, crop.height), (8, 8));
    let bytes = STANDARD.decode(crop.png_base64).unwrap();
    let mut reader = png::Decoder::new(Cursor::new(bytes)).read_info().unwrap();
    let mut pixels = vec![0; reader.output_buffer_size()];
    reader.next_frame(&mut pixels).unwrap();
    assert_eq!(&pixels[..3], &[255; 3]);
    assert_eq!(&pixels[(4 * 8 + 4) * 3..(4 * 8 + 5) * 3], &[0; 3]);
}

#[test]
fn crop_masks_control_values_before_encoding() {
    let pixels = vec![255; 8 * 8 * 3];
    let bounds = ElementBox {
        x: 0,
        y: 0,
        width: 8,
        height: 8,
    };
    let mask = Rect {
        x: 4.0,
        y: 4.0,
        width: 2.0,
        height: 2.0,
    };
    let (width, height, cropped) = crop_rgb(&pixels, 8, 8, viewport(), bounds, &[mask]).unwrap();
    assert_eq!((width, height), (8, 8));
    assert_eq!(&cropped[..3], &[255, 255, 255]);
    assert_eq!(&cropped[(4 * 8 + 4) * 3..(4 * 8 + 4) * 3 + 3], &[0, 0, 0]);
    let png = encode(width, height, &cropped).unwrap();
    assert!(png.len() <= 4 * 1024 * 1024);
    let decoder = png::Decoder::new(std::io::Cursor::new(png));
    let mut reader = decoder.read_info().unwrap();
    let mut decoded = vec![0; reader.output_buffer_size()];
    reader.next_frame(&mut decoded).unwrap();
    assert_eq!(decoded, cropped);
}

#[test]
fn crop_is_at_most_1024_each_way_and_refuses_invalid_or_unbounded_inputs() {
    let mut viewport = viewport();
    viewport.width = 2048.0;
    viewport.client_width = 2048.0;
    let bounds = ElementBox {
        x: 0,
        y: 0,
        width: 2048,
        height: 8,
    };
    let pixels = vec![255; 2048 * 8 * 3];
    let (width, height, _) = crop_rgb(&pixels, 2048, 8, viewport, bounds, &[]).unwrap();
    assert_eq!((width, height), (1024, 4));
    let mask = Rect {
        x: f64::NAN,
        y: 0.0,
        width: 2.0,
        height: 2.0,
    };
    assert!(crop_rgb(&pixels, 2048, 8, viewport, bounds, &[mask]).is_err());
    assert!(
        crop_rgb(
            &pixels,
            2048,
            8,
            viewport,
            bounds,
            &vec![
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 2.0,
                    height: 2.0
                };
                513
            ]
        )
        .is_err()
    );
    assert!(crop_rgb(&pixels[..10], 2048, 8, viewport, bounds, &[]).is_err());
    assert!(crop_rgb(&pixels, 100_000, 100_000, viewport, bounds, &[]).is_err());
}
