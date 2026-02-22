use crate::FrameFormat;

/// Unified error type returned by all rcam operations.
#[derive(Debug, thiserror::Error)]
pub enum CameraError {
    #[error("No camera device found")]
    NoCameraFound,

    #[error("Permission denied")]
    PermissionDenied,

    #[error("Device already in use")]
    DeviceBusy,

    #[error("Unsupported format: {0:?}")]
    UnsupportedFormat(FrameFormat),

    #[error("Recording has not been started")]
    NotRecording,

    #[error("Camera stream is not active")]
    StreamNotActive,

    #[error("Backend error: {0}")]
    Backend(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Platform not supported")]
    Unsupported,
}
