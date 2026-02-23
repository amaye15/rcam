use crate::{
    CameraCapabilities, CameraConfig, CameraError, CameraInfo, Frame, RecordingOutput, VideoData,
};

/// The core abstraction for a camera device.
///
/// Implemented by each platform backend. Users interact exclusively with this
/// trait (or the [`crate::Camera`] type alias that points to the active backend).
///
/// # Async runtime
/// All async methods are runtime-agnostic. On WASM only a WASM-compatible
/// executor (e.g. `wasm-bindgen-futures`) may be used, and the returned
/// futures are **not** required to be `Send` (because browser APIs are
/// inherently single-threaded).
///
/// # Conditional `Send + Sync`
/// On native targets the trait requires `Send + Sync` so cameras can be
/// shared across threads. On WASM those bounds are omitted because
/// `web_sys` objects are not `Send`.

// ---------------------------------------------------------------------------
// Native targets (not WASM) — futures are Send + Sync
// ---------------------------------------------------------------------------
#[cfg(not(target_arch = "wasm32"))]
#[async_trait::async_trait]
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

// ---------------------------------------------------------------------------
// WASM — futures do NOT need to be Send (single JS thread)
// ---------------------------------------------------------------------------
#[cfg(target_arch = "wasm32")]
#[async_trait::async_trait(?Send)]
pub trait CameraDevice {
    async fn enumerate() -> Result<Vec<CameraInfo>, CameraError>
    where
        Self: Sized;

    async fn open(config: CameraConfig) -> Result<Self, CameraError>
    where
        Self: Sized;

    async fn start_stream(&mut self) -> Result<(), CameraError>;

    async fn capture_frame(&self) -> Result<Frame, CameraError>;

    async fn take_photo(&self) -> Result<Frame, CameraError>;

    async fn start_recording(&mut self, output: RecordingOutput) -> Result<(), CameraError>;

    async fn stop_recording(&mut self) -> Result<VideoData, CameraError>;

    async fn stop_stream(&mut self) -> Result<(), CameraError>;

    fn capabilities(&self) -> &CameraCapabilities;

    async fn close(self) -> Result<(), CameraError>
    where
        Self: Sized;
}
