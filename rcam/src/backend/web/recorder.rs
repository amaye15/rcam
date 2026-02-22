//! Browser MediaRecorder-based video recording (Phase 1 stub).
//!
//! Phase 3 implementation will:
//!   1. Create `MediaRecorder::new_with_media_stream(&stream)`.
//!   2. Attach a `dataavailable` event handler that pushes chunks into a buffer.
//!   3. On `stop`, combine all chunks into a single `Vec<u8>` and return
//!      `VideoOutput::Buffer` (no filesystem access in browsers).
//!
//! WASM-specific note: recording always returns `VideoOutput::Buffer`; the
//! `RecordingOutput::File` variant is rejected with `CameraError::Unsupported`.

use crate::CameraError;

pub struct WebRecorder;

impl WebRecorder {
    pub fn new() -> Result<Self, CameraError> {
        Ok(Self)
    }

    pub async fn stop(self) -> Result<Vec<u8>, CameraError> {
        Err(CameraError::Unsupported)
    }
}
