pub mod error;
pub mod traits;
pub mod types;

mod backend;

pub use error::CameraError;
pub use traits::CameraDevice;
pub use types::{
    CameraCapabilities, CameraConfig, CameraInfo, CameraPosition, Frame, FrameFormat,
    RecordingOutput, Resolution, VideoData, VideoOutput,
};

// ---------------------------------------------------------------------------
// Compile-time platform backend selection
// ---------------------------------------------------------------------------
//
// The `Camera` type alias resolves to the correct backend at build time.
// End-users import `rcam::Camera` and never touch platform-specific types.

#[cfg(any(target_os = "ios", target_os = "macos"))]
pub type Camera = backend::avfoundation::AvfCamera;

#[cfg(target_os = "android")]
pub type Camera = backend::android::AndroidCamera;

#[cfg(target_os = "linux")]
pub type Camera = backend::v4l2::V4l2Camera;

#[cfg(target_os = "windows")]
pub type Camera = backend::mediafoundation::MfCamera;

#[cfg(target_arch = "wasm32")]
pub type Camera = backend::web::WebCamera;

// Fail fast on unsupported platforms.
#[cfg(not(any(
    target_os = "ios",
    target_os = "macos",
    target_os = "android",
    target_os = "linux",
    target_os = "windows",
    target_arch = "wasm32",
)))]
compile_error!(
    "rcam does not support this platform. \
     Supported targets: iOS, macOS, Android, Linux, Windows, wasm32."
);
