//! Media Foundation frame capture and device enumeration (Phase 1 stub).
//!
//! Phase 2 implementation will:
//!   1. Call `DeviceInformation::FindAllAsync()` filtered by video capture panel.
//!   2. Initialise `MediaCapture` with `MediaCaptureInitializationSettings`.
//!   3. Use `LowLagPhotoCapture` for still photos.
//!   4. Use `LowLagMediaRecording` for video recording.
//!   5. Map WinRT async operations to Rust futures.

use crate::{CameraError, CameraInfo};

pub fn enumerate_devices() -> Result<Vec<CameraInfo>, CameraError> {
    // Phase 2: use windows-rs Media Foundation APIs.
    Ok(vec![])
}
