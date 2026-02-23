//! Tests for the `image-output` feature — `Frame::to_image()`.
//!
//! This file is only compiled when the crate is built with
//! `--features image-output` (declared via `required-features` in Cargo.toml).

use image::{DynamicImage, ImageBuffer, Rgb};
use rcam::{Frame, FrameFormat};

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

fn make_frame(w: u32, h: u32, format: FrameFormat, data: Vec<u8>) -> Frame {
    Frame {
        data,
        width: w,
        height: h,
        format,
        timestamp_us: 0,
    }
}

fn solid_rgb24(w: u32, h: u32, r: u8, g: u8, b: u8) -> Vec<u8> {
    std::iter::repeat([r, g, b])
        .take((w * h) as usize)
        .flatten()
        .collect()
}

fn solid_bgra(w: u32, h: u32, b: u8, g: u8, r: u8, a: u8) -> Vec<u8> {
    std::iter::repeat([b, g, r, a])
        .take((w * h) as usize)
        .flatten()
        .collect()
}

fn gray_yuv420(w: u32, h: u32) -> Vec<u8> {
    // Y=128 (mid-gray), U=128, V=128 → neutral colour, near-gray RGB output.
    let y_size = (w * h) as usize;
    let uv_size = (w as usize / 2) * (h as usize / 2);
    vec![0x80u8; y_size + 2 * uv_size]
}

fn gray_nv12(w: u32, h: u32) -> Vec<u8> {
    let y_size = (w * h) as usize;
    let uv_size = (w * h / 2) as usize;
    vec![0x80u8; y_size + uv_size]
}

/// Encode a small solid-colour RGB image as JPEG bytes.
fn encode_jpeg(w: u32, h: u32, r: u8, g: u8, b: u8) -> Vec<u8> {
    let img = ImageBuffer::<Rgb<u8>, _>::from_fn(w, h, |_, _| Rgb([r, g, b]));
    let dynimg = DynamicImage::ImageRgb8(img);
    let mut buf = Vec::new();
    dynimg
        .write_to(
            &mut std::io::Cursor::new(&mut buf),
            image::ImageFormat::Jpeg,
        )
        .expect("JPEG encode failed");
    buf
}

// ---------------------------------------------------------------------------
// RGB24
// ---------------------------------------------------------------------------

#[test]
fn rgb24_correct_dimensions() {
    let (w, h) = (16, 12);
    let frame = make_frame(w, h, FrameFormat::RGB24, solid_rgb24(w, h, 255, 0, 0));
    let img = frame.to_image().unwrap();
    assert_eq!(img.width(), w);
    assert_eq!(img.height(), h);
}

#[test]
fn rgb24_colour_is_preserved() {
    let (w, h) = (4, 4);
    let frame = make_frame(w, h, FrameFormat::RGB24, solid_rgb24(w, h, 200, 100, 50));
    let img = frame.to_image().unwrap();
    let rgb = img.to_rgb8();
    let px = rgb.get_pixel(0, 0);
    assert_eq!(px[0], 200);
    assert_eq!(px[1], 100);
    assert_eq!(px[2], 50);
}

#[test]
fn rgb24_too_small_buffer_returns_error() {
    let frame = make_frame(640, 480, FrameFormat::RGB24, vec![0u8; 10]);
    assert!(frame.to_image().is_err());
}

// ---------------------------------------------------------------------------
// BGRA
// ---------------------------------------------------------------------------

#[test]
fn bgra_channels_are_reordered_to_rgba() {
    let (w, h) = (4, 4);
    // BGRA = [B=0x10, G=0x20, R=0x30, A=0xFF]
    let frame = make_frame(
        w,
        h,
        FrameFormat::BGRA,
        solid_bgra(w, h, 0x10, 0x20, 0x30, 0xFF),
    );
    let img = frame.to_image().unwrap();
    let rgba = img.to_rgba8();
    let px = rgba.get_pixel(0, 0);
    // After reorder: R=0x30, G=0x20, B=0x10, A=0xFF
    assert_eq!(px[0], 0x30, "R channel");
    assert_eq!(px[1], 0x20, "G channel");
    assert_eq!(px[2], 0x10, "B channel");
    assert_eq!(px[3], 0xFF, "A channel");
}

#[test]
fn bgra_correct_dimensions() {
    let (w, h) = (8, 6);
    let frame = make_frame(w, h, FrameFormat::BGRA, solid_bgra(w, h, 0, 128, 200, 255));
    let img = frame.to_image().unwrap();
    assert_eq!(img.width(), w);
    assert_eq!(img.height(), h);
}

#[test]
fn bgra_too_small_buffer_returns_error() {
    let frame = make_frame(640, 480, FrameFormat::BGRA, vec![0u8; 10]);
    assert!(frame.to_image().is_err());
}

// ---------------------------------------------------------------------------
// YUV420 (planar I420)
// ---------------------------------------------------------------------------

#[test]
fn yuv420_gray_frame_produces_valid_image() {
    let (w, h) = (8, 8);
    let frame = make_frame(w, h, FrameFormat::YUV420, gray_yuv420(w, h));
    let img = frame.to_image().unwrap();
    assert_eq!(img.width(), w);
    assert_eq!(img.height(), h);
    // All pixels should be near-gray (within ±10 of 128).
    let rgb = img.to_rgb8();
    for px in rgb.pixels() {
        for &channel in px.0.iter() {
            let diff = (channel as i32 - 128).unsigned_abs();
            assert!(
                diff <= 10,
                "expected near-gray pixel, got channel value {channel}"
            );
        }
    }
}

#[test]
fn yuv420_too_small_buffer_returns_error() {
    let frame = make_frame(640, 480, FrameFormat::YUV420, vec![0u8; 10]);
    assert!(frame.to_image().is_err());
}

// ---------------------------------------------------------------------------
// NV12 (semi-planar)
// ---------------------------------------------------------------------------

#[test]
fn nv12_gray_frame_produces_valid_image() {
    let (w, h) = (8, 8);
    let frame = make_frame(w, h, FrameFormat::NV12, gray_nv12(w, h));
    let img = frame.to_image().unwrap();
    assert_eq!(img.width(), w);
    assert_eq!(img.height(), h);
    let rgb = img.to_rgb8();
    for px in rgb.pixels() {
        for &channel in px.0.iter() {
            let diff = (channel as i32 - 128).unsigned_abs();
            assert!(
                diff <= 10,
                "expected near-gray pixel, got channel value {channel}"
            );
        }
    }
}

#[test]
fn nv12_too_small_buffer_returns_error() {
    let frame = make_frame(640, 480, FrameFormat::NV12, vec![0u8; 10]);
    assert!(frame.to_image().is_err());
}

// ---------------------------------------------------------------------------
// MJPEG
// ---------------------------------------------------------------------------

#[test]
fn mjpeg_valid_jpeg_decodes_correctly() {
    let (w, h) = (8, 8);
    let jpeg = encode_jpeg(w, h, 180, 90, 50);
    let frame = make_frame(w, h, FrameFormat::MJPEG, jpeg);
    let img = frame.to_image().unwrap();
    // JPEG is lossy — just check dimensions and that no error was returned.
    assert_eq!(img.width(), w);
    assert_eq!(img.height(), h);
}

#[test]
fn mjpeg_invalid_bytes_return_error() {
    let frame = make_frame(4, 4, FrameFormat::MJPEG, vec![0xDE, 0xAD, 0xBE, 0xEF]);
    assert!(frame.to_image().is_err());
}
