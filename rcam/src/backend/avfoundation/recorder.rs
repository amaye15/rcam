//! AVAssetWriter-based video recording (Phase 1 stub).
//!
//! Phase 2 will use `AVAssetWriter` + `AVAssetWriterInput` to mux H.264/HEVC
//! frames into an `.mp4` container, or `AVCaptureMovieFileOutput` as a simpler
//! alternative.

use std::path::PathBuf;

use crate::CameraError;

/// Manages an `AVAssetWriter` recording session.
pub struct AvfRecorder {
    _output_path: PathBuf,
}

impl AvfRecorder {
    pub fn new(path: PathBuf) -> Result<Self, CameraError> {
        Ok(Self { _output_path: path })
    }

    pub fn stop(self) -> Result<PathBuf, CameraError> {
        Err(CameraError::Unsupported)
    }
}
