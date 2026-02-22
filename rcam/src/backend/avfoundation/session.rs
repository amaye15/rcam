//! AVCaptureSession management (Phase 1 stub).
//!
//! Phase 2 will use `objc2-av-foundation` to create and manage an
//! `AVCaptureSession`, add inputs/outputs, and start/stop running.

use crate::{CameraConfig, CameraError};

/// Wraps an `AVCaptureSession` lifecycle.
pub struct AvfSession {
    // Phase 2: hold objc2 AVCaptureSession handle.
    _config: CameraConfig,
}

impl AvfSession {
    pub fn new(config: &CameraConfig) -> Result<Self, CameraError> {
        Ok(Self { _config: config.clone() })
    }
}
