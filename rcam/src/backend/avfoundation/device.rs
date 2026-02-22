//! AVCaptureDevice enumeration (Phase 1 stub).
//!
//! Phase 2 will call `AVCaptureDevice::devices_with_media_type(AVMediaTypeVideo)`
//! via `objc2-av-foundation` to return real hardware devices.

use crate::{CameraError, CameraInfo};

/// Enumerate connected cameras via AVFoundation.
pub fn enumerate_devices() -> Result<Vec<CameraInfo>, CameraError> {
    // Phase 2: call objc2-av-foundation APIs here.
    Ok(vec![])
}
