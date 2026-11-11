use super::prompt_image::{
    base64_len, fit_base64_for_send, fit_within, prepare, prepare_data_url, MAX_BASE64_BYTES,
    MAX_DIMENSION,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use image::{DynamicImage, GenericImageView, ImageFormat, Rgb, RgbImage, Rgba, RgbaImage};
use std::io::Cursor;

fn encode(image: &DynamicImage, format: ImageFormat) -> Vec<u8> {
    let mut buffer = Vec::new();
    image.write_to(&mut Cursor::new(&mut buffer), format).unwrap();
    buffer
}

fn gradient(width: u32, height: u32) -> DynamicImage {
    DynamicImage::ImageRgb8(RgbImage::from_fn(width, height, |x, y| {
        Rgb([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8])
    }))
}

/// 伪随机噪点：PNG 几乎无法压缩，用来触发字节预算。
fn noise(width: u32, height: u32) -> DynamicImage {
    let mut state: u32 = 0x1234_5678;
    DynamicImage::ImageRgb8(RgbImage::from_fn(width, height, |_, _| {
        let mut next = || {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 24) as u8
        };
        Rgb([next(), next(), next()])
    }))
}

fn decode(prepared_base64: &str) -> DynamicImage {
    image::load_from_memory(&STANDARD.decode(prepared_base64).unwrap()).unwrap()
}

#[test]
fn small_supported_image_keeps_original_bytes() {
    let bytes = encode(&gradient(320, 200), ImageFormat::Png);
    let image = prepare(&bytes).unwrap();

    assert!(!image.reencoded);
    assert_eq!(image.mime, "image/png");
    assert_eq!(STANDARD.decode(&image.base64).unwrap(), bytes);
    assert_eq!((image.width, image.height), (320, 200));
}

#[test]
fn oversized_dimensions_scale_long_edge_to_limit() {
    let bytes = encode(&gradient(4000, 1000), ImageFormat::Png);
    let image = prepare(&bytes).unwrap();

    assert!(image.reencoded);
    assert_eq!((image.source_width, image.source_height), (4000, 1000));
    assert_eq!((image.width, image.height), (MAX_DIMENSION, 500));
    assert_eq!(decode(&image.base64).dimensions(), (MAX_DIMENSION, 500));
    assert!(image.base64.len() <= MAX_BASE64_BYTES);
}

#[test]
fn oversized_bytes_within_dimensions_fall_back_to_jpeg() {
    let bytes = encode(&noise(1600, 1600), ImageFormat::Png);
    assert!(base64_len(bytes.len()) > MAX_BASE64_BYTES, "fixture must exceed the byte budget");

    let image = prepare(&bytes).unwrap();

    assert!(image.reencoded);
    assert_eq!(image.mime, "image/jpeg");
    assert!(image.base64.len() <= MAX_BASE64_BYTES);
}

#[test]
fn transparent_image_stays_png() {
    let transparent = DynamicImage::ImageRgba8(RgbaImage::from_fn(3000, 100, |x, _| {
        Rgba([10, 20, 30, if x < 1500 { 0 } else { 255 }])
    }));
    let image = prepare(&encode(&transparent, ImageFormat::Png)).unwrap();

    assert_eq!(image.mime, "image/png");
    assert_eq!(image.width, MAX_DIMENSION);
    assert!(decode(&image.base64).color().has_alpha());
}

#[test]
fn unsupported_format_is_converted() {
    let bytes = encode(&gradient(64, 64), ImageFormat::Bmp);
    let image = prepare(&bytes).unwrap();

    assert!(image.reencoded);
    assert!(matches!(image.mime, "image/png" | "image/jpeg"));
    assert_eq!(decode(&image.base64).dimensions(), (64, 64));
}

#[test]
fn format_comes_from_content_not_declared_mime() {
    let jpeg = encode(&gradient(64, 64), ImageFormat::Jpeg);
    let data_url = format!("data:image/png;base64,{}", STANDARD.encode(&jpeg));
    let image = prepare_data_url(&data_url).unwrap();

    assert!(!image.reencoded);
    assert_eq!(image.mime, "image/jpeg");
}

#[test]
fn non_image_input_is_rejected() {
    assert!(prepare(b"not an image").is_err());
    assert!(prepare_data_url("data:image/png,plain").is_err());
}

#[test]
fn send_fallback_leaves_compliant_images_alone() {
    let b64 = STANDARD.encode(encode(&gradient(100, 100), ImageFormat::Png));
    assert!(fit_base64_for_send("image/png", &b64).is_none());
}

#[test]
fn send_fallback_fixes_mislabelled_and_oversized_images() {
    let jpeg = STANDARD.encode(encode(&gradient(100, 100), ImageFormat::Jpeg));
    let (mime, b64) = fit_base64_for_send("image/png", &jpeg).unwrap();
    assert_eq!(mime, "image/jpeg");
    assert_eq!(b64, jpeg);

    let large = STANDARD.encode(encode(&gradient(2600, 1300), ImageFormat::Png));
    let (_, resized) = fit_base64_for_send("image/png", &large).unwrap();
    assert_eq!(decode(&resized).dimensions(), (MAX_DIMENSION, 1000));
    // 第二次命中缓存，结果一致
    assert_eq!(fit_base64_for_send("image/png", &large).unwrap().1, resized);
}

#[test]
fn send_fallback_passes_through_undecodable_payloads() {
    assert!(fit_base64_for_send("image/png", "%%%").is_none());
    assert!(fit_base64_for_send("image/png", &STANDARD.encode(b"garbage")).is_none());
}

#[test]
fn fit_within_keeps_aspect_ratio() {
    assert_eq!(fit_within(1000, 800, 2000), (1000, 800));
    assert_eq!(fit_within(4000, 3000, 2000), (2000, 1500));
    assert_eq!(fit_within(1000, 5000, 2000), (400, 2000));
    assert_eq!(fit_within(100_000, 1, 2000), (2000, 1));
}
