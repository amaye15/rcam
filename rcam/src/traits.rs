use async_trait::async_trait;

use crate::{
    CameraCapabilities, CameraConfig, CameraError, CameraInfo, Frame, RecordingOutput, VideoData,
};

/// The core abstraction for a camera device.
///
/// Implemented by each platform backend. Users interact exclusively with this
/// trait (or the [`crate::Camera`] type alias that points to the active backend).
///
/// # Async runtime
/// All async methods are runtime-agnostic except on WASM, where only a
/// WASM-compatible executor (e.g. `wasm-bindgen-futures`) may be used.
#[async_trait]
pub trait CameraDevice: Send + Sync {
    /// List all cameras available on the current system.
    async fn enumerate() -> Result<Vec<CameraInfo>, CameraError>
    where
        Self: Sized;

    /// Open a camera using the provided configuration.
    ///
    /// Passing `CameraConfig::default()` opens the system-default camera at
    /// 720p / 30 fps.
    async fn open(config: CameraConfig) -> Result<Self, CameraError>
    where
        Self: Sized;

    /// Begin continuous frame delivery.
    async fn start_stream(&mut self) -> Result<(), CameraError>;

    /// Grab the most recent frame from the active stream.
    async fn capture_frame(&self) -> Result<Frame, CameraError>;

    /// Capture a single still image at the highest available quality.
    async fn take_photo(&self) -> Result<Frame, CameraError>;

    /// Start recording video to `output`.
    async fn start_recording(&mut self, output: RecordingOutput) -> Result<(), CameraError>;

    /// Stop recording and return the finished video.
    async fn stop_recording(&mut self) -> Result<VideoData, CameraError>;

    /// Stop the active frame stream.
    async fn stop_stream(&mut self) -> Result<(), CameraError>;

    /// Query what this device supports (resolutions, formats, etc.).
    fn capabilities(&self) -> &CameraCapabilities;

    /// Close the device and release all hardware resources.
    async fn close(self) -> Result<(), CameraError>
    where
        Self: Sized;
}
