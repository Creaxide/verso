//! Stage 1 image tests: round-trips, alpha flattening, resizing and typed errors.

use image::{ExtendedColorType, ImageEncoder};
use verso_core::{convert, image_info, Format, ImageOptions, Options};

fn rgba_png(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(pixels, width, height, ExtendedColorType::Rgba8)
        .expect("encode fixture");
    bytes
}

fn solid(width: u32, height: u32) -> Vec<u8> {
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for _ in 0..width * height {
        pixels.extend_from_slice(&[40, 90, 160, 255]);
    }
    rgba_png(width, height, &pixels)
}

fn options() -> Options {
    Options::default()
}

#[test]
fn png_jpg_png_round_trip_keeps_dimensions() {
    let png = solid(37, 19);
    let jpg = convert(Format::Png, Format::Jpg, &png, &options()).expect("png to jpg");
    let back = convert(Format::Jpg, Format::Png, &jpg, &options()).expect("jpg to png");
    let info = image_info(&back).expect("read dimensions");
    assert_eq!((info.width, info.height), (37, 19));
}

#[test]
fn transparent_pixels_become_white_in_jpg() {
    // A fully transparent 32 × 32 image: flattening must make it white, and a
    // centre sample stays away from JPEG block edges.
    let mut pixels = Vec::with_capacity(32 * 32 * 4);
    for _ in 0..32 * 32 {
        pixels.extend_from_slice(&[255, 0, 0, 0]);
    }
    let png = rgba_png(32, 32, &pixels);

    let jpg = convert(Format::Png, Format::Jpg, &png, &options()).expect("png to jpg");
    let decoded = image::load_from_memory_with_format(&jpg, image::ImageFormat::Jpeg)
        .expect("decode jpg")
        .to_rgb8();
    let pixel = decoded.get_pixel(16, 16);
    assert!(
        pixel[0] > 250 && pixel[1] > 250 && pixel[2] > 250,
        "expected white, got {pixel:?}"
    );
}

#[test]
fn max_dimension_scales_down_and_keeps_aspect_ratio() {
    let png = solid(800, 400);
    let options = Options {
        image: ImageOptions {
            max_dimension: Some(200),
            ..ImageOptions::default()
        },
        ..Options::default()
    };
    let resized = convert(Format::Png, Format::Png, &png, &options).expect("resize");
    let info = image_info(&resized).expect("dimensions");
    assert_eq!((info.width, info.height), (200, 100));
}

#[test]
fn corrupt_input_returns_a_typed_error() {
    let error = convert(Format::Png, Format::Jpg, b"not a png at all", &options())
        .expect_err("must refuse");
    assert_eq!(error.code(), "parse");
}

#[test]
fn gif_and_bmp_targets_encode() {
    let png = solid(24, 16);
    for target in [Format::Webp, Format::Bmp, Format::Gif] {
        let bytes = convert(Format::Png, target, &png, &options())
            .unwrap_or_else(|error| panic!("png to {target:?}: {error}"));
        let info = image_info(&bytes).unwrap_or_else(|error| panic!("{target:?}: {error}"));
        assert_eq!((info.width, info.height), (24, 16), "{target:?}");
    }
}

#[test]
fn quality_changes_jpg_size() {
    let png = solid(200, 200);
    let low = Options {
        image: ImageOptions {
            quality: 20,
            ..ImageOptions::default()
        },
        ..Options::default()
    };
    let high = Options {
        image: ImageOptions {
            quality: 95,
            ..ImageOptions::default()
        },
        ..Options::default()
    };
    let small = convert(Format::Png, Format::Jpg, &png, &low).expect("low quality");
    let large = convert(Format::Png, Format::Jpg, &png, &high).expect("high quality");
    assert!(small.len() < large.len());
}

#[test]
fn same_format_conversion_still_produces_an_image() {
    let png = solid(12, 9);
    let again = convert(Format::Png, Format::Png, &png, &options()).expect("png to png");
    assert!(again.starts_with(b"\x89PNG"));
    let info = image_info(&again).expect("dimensions");
    assert_eq!((info.width, info.height), (12, 9));
}
