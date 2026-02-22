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
            resolution: Resolution { width: 1280, height: 720 },
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
