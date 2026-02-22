//! AMediaRecorder / AMediaMuxer video recording (Phase 1 stub).
//!
//! Phase 5 implementation will use `AMediaRecorder` to encode H.264 video
//! and `AMediaMuxer` to write an MP4 container to disk.

use std::path::PathBuf;

use crate::CameraError;

pub struct AndroidRecorder {
    _output: PathBuf,
}

impl AndroidRecorder {
    pub fn new(output: PathBuf) -> Result<Self, CameraError> {
        Ok(Self { _output: output })
    }

    pub fn stop(self) -> Result<PathBuf, CameraError> {
        Err(CameraError::Unsupported)
    }
}
