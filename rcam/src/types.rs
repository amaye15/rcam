use std::path::PathBuf;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Information about a discovered camera device.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CameraInfo {
    /// Platform-specific unique identifier.
    pub id: String,
    /// Human-readable device name.
    pub name: String,
    /// Physical position of the camera.
    pub position: CameraPosition,
    /// Whether this is the system-default camera.
    pub is_default: bool,
}

/// Configuration used when opening a camera.
#[derive(Debug, Clone)]
pub struct CameraConfig {
    /// Target device; `None` selects the system default.
    pub device_id: Option<String>,
    pub resolution: Resolution,
    pub frame_rate: u32,
    pub format: FrameFormat,
    pub position: CameraPosition,
}

impl Default for CameraConfig {
    fn default() -> Self {
        Self {
            device_id: None,
            resolution: Resolution {
                width: 1280,
                height: 720,
            },
            frame_rate: 30,
            format: FrameFormat::NV12,
            position: CameraPosition::Unknown,
        }
    }
}

/// A single captured video frame.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Frame {
    /// Raw pixel data in the layout described by `format`.
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub format: FrameFormat,
    /// Capture timestamp in microseconds (platform epoch varies).
    pub timestamp_us: u64,
}

/// The result of a completed video recording.
#[derive(Debug)]
pub struct VideoData {
    pub kind: VideoOutput,
}

/// Where recorded video data resides.
#[derive(Debug)]
pub enum VideoOutput {
    /// Recorded to a file on disk.
    File(PathBuf),
    /// Held in memory (WASM / short clips).
    Buffer(Vec<u8>),
}

/// Pixel dimensions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Resolution {
    pub width: u32,
    pub height: u32,
}

/// Physical position of a camera relative to the device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum CameraPosition {
    Front,
    Back,
    External,
    Unknown,
}

/// Pixel format / encoding of raw frame data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum FrameFormat {
    /// Motion-JPEG compressed frames.
    MJPEG,
    /// Y-plane followed by interleaved UV plane (semi-planar YCbCr 4:2:0).
    NV12,
    /// Planar YCbCr 4:2:0.
    YUV420,
    /// 32-bit BGRA packed pixels.
    BGRA,
    /// 24-bit RGB packed pixels.
    RGB24,
}

/// Where to direct a video recording.
#[derive(Debug, Clone)]
pub enum RecordingOutput {
    /// Write to a file path on disk.
    File(PathBuf),
    /// Accumulate in memory (the only option on WASM).
    Buffer,
}

/// Capabilities reported by an open camera device.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CameraCapabilities {
    pub supported_resolutions: Vec<Resolution>,
    pub supported_frame_rates: Vec<u32>,
    pub supported_formats: Vec<FrameFormat>,
    pub has_torch: bool,
    pub has_zoom: bool,
}

// ---------------------------------------------------------------------------
// image-output feature: Frame → image::DynamicImage
// ---------------------------------------------------------------------------

#[cfg(feature = "image-output")]
impl Frame {
    /// Convert this frame to an [`image::DynamicImage`].
    ///
    /// Supported formats: `RGB24`, `BGRA`, `MJPEG`, `YUV420`, `NV12`.
    /// Returns [`crate::CameraError::Backend`] if the pixel buffer is
    /// malformed or a JPEG decode fails.
    pub fn to_image(&self) -> Result<image::DynamicImage, crate::CameraError> {
        use image::{DynamicImage, ImageBuffer, Rgb, Rgba};

        let w = self.width;
        let h = self.height;

        match self.format {
            FrameFormat::RGB24 => ImageBuffer::<Rgb<u8>, _>::from_raw(w, h, self.data.clone())
                .map(DynamicImage::ImageRgb8)
                .ok_or_else(|| {
                    crate::CameraError::Backend(
                        "RGB24 buffer size does not match declared dimensions".into(),
                    )
                }),
            FrameFormat::BGRA => {
                // BGRA → RGBA channel reorder.
                let rgba: Vec<u8> = self
                    .data
                    .chunks_exact(4)
                    .flat_map(|p| [p[2], p[1], p[0], p[3]])
                    .collect();
                ImageBuffer::<Rgba<u8>, _>::from_raw(w, h, rgba)
                    .map(DynamicImage::ImageRgba8)
                    .ok_or_else(|| {
                        crate::CameraError::Backend(
                            "BGRA buffer size does not match declared dimensions".into(),
                        )
                    })
            }
            FrameFormat::MJPEG => image::load_from_memory(&self.data)
                .map_err(|e| crate::CameraError::Backend(format!("JPEG decode error: {e}"))),
            FrameFormat::YUV420 => yuv420_to_image(w, h, &self.data),
            FrameFormat::NV12 => nv12_to_image(w, h, &self.data),
        }
    }
}

/// BT.601 YCbCr → RGB helper used by YUV420 and NV12 conversions.
#[cfg(feature = "image-output")]
#[inline]
fn yuv_to_rgb(y: u8, u: u8, v: u8) -> [u8; 3] {
    let y = y as i32;
    let u = u as i32 - 128;
    let v = v as i32 - 128;
    let r = (y + 1_402 * v / 1_000).clamp(0, 255) as u8;
    let g = (y - 344 * u / 1_000 - 714 * v / 1_000).clamp(0, 255) as u8;
    let b = (y + 1_772 * u / 1_000).clamp(0, 255) as u8;
    [r, g, b]
}

/// Convert planar I420 (YUV 4:2:0) to an RGB image.
#[cfg(feature = "image-output")]
fn yuv420_to_image(w: u32, h: u32, data: &[u8]) -> Result<image::DynamicImage, crate::CameraError> {
    use image::{DynamicImage, ImageBuffer, Rgb};

    let y_size = (w * h) as usize;
    let uv_size = (w as usize / 2) * (h as usize / 2);
    if data.len() < y_size + 2 * uv_size {
        return Err(crate::CameraError::Backend(
            "YUV420 buffer too small".into(),
        ));
    }
    let y_plane = &data[..y_size];
    let u_plane = &data[y_size..y_size + uv_size];
    let v_plane = &data[y_size + uv_size..y_size + 2 * uv_size];

    let stride = w as usize;
    let chroma_stride = stride / 2;
    let mut rgb = vec![0u8; y_size * 3];
    for row in 0..h as usize {
        for col in 0..w as usize {
            let y = y_plane[row * stride + col];
            let u = u_plane[(row / 2) * chroma_stride + col / 2];
            let v = v_plane[(row / 2) * chroma_stride + col / 2];
            let pix = yuv_to_rgb(y, u, v);
            let off = (row * stride + col) * 3;
            rgb[off..off + 3].copy_from_slice(&pix);
        }
    }
    ImageBuffer::<Rgb<u8>, _>::from_raw(w, h, rgb)
        .map(DynamicImage::ImageRgb8)
        .ok_or_else(|| crate::CameraError::Backend("YUV420 conversion error".into()))
}

/// Convert semi-planar NV12 (Y + interleaved UV) to an RGB image.
#[cfg(feature = "image-output")]
fn nv12_to_image(w: u32, h: u32, data: &[u8]) -> Result<image::DynamicImage, crate::CameraError> {
    use image::{DynamicImage, ImageBuffer, Rgb};

    let y_size = (w * h) as usize;
    let uv_size = (w * h / 2) as usize;
    if data.len() < y_size + uv_size {
        return Err(crate::CameraError::Backend("NV12 buffer too small".into()));
    }
    let y_plane = &data[..y_size];
    let uv_plane = &data[y_size..y_size + uv_size];

    let stride = w as usize;
    let mut rgb = vec![0u8; y_size * 3];
    for row in 0..h as usize {
        for col in 0..w as usize {
            let y = y_plane[row * stride + col];
            // UV pairs are indexed by the even column of the chroma row.
            let uv_base = (row / 2) * stride + (col & !1);
            let u = uv_plane[uv_base];
            let v = uv_plane[uv_base + 1];
            let pix = yuv_to_rgb(y, u, v);
            let off = (row * stride + col) * 3;
            rgb[off..off + 3].copy_from_slice(&pix);
        }
    }
    ImageBuffer::<Rgb<u8>, _>::from_raw(w, h, rgb)
        .map(DynamicImage::ImageRgb8)
        .ok_or_else(|| crate::CameraError::Backend("NV12 conversion error".into()))
}
