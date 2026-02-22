//! Web (WASM) backend using `getUserMedia` and `MediaRecorder`.
//!
//! Phase 3 will implement full browser camera access via `web-sys`.

pub mod capture;
pub mod recorder;

use async_trait::async_trait;

use crate::{
    CameraCapabilities, CameraConfig, CameraError, CameraInfo, Frame, RecordingOutput, VideoData,
};
use crate::traits::CameraDevice;

/// WASM camera handle backed by `navigator.mediaDevices.getUserMedia`.
pub struct WebCamera {
    capabilities: CameraCapabilities,
}

#[async_trait]
impl CameraDevice for WebCamera {
    async fn enumerate() -> Result<Vec<CameraInfo>, CameraError>
    where
        Self: Sized,
    {
        capture::enumerate_devices().await
    }

    async fn open(_config: CameraConfig) -> Result<Self, CameraError>
    where
        Self: Sized,
    {
        Ok(Self {
            capabilities: CameraCapabilities {
                supported_resolutions: vec![],
                supported_frame_rates: vec![],
                supported_formats: vec![],
                has_torch: false,
                has_zoom: false,
            },
        })
    }

    async fn start_stream(&mut self) -> Result<(), CameraError> {
        Err(CameraError::Unsupported)
    }

    async fn capture_frame(&self) -> Result<Frame, CameraError> {
        Err(CameraError::Unsupported)
    }

    async fn take_photo(&self) -> Result<Frame, CameraError> {
        Err(CameraError::Unsupported)
    }

    async fn start_recording(&mut self, _output: RecordingOutput) -> Result<(), CameraError> {
        Err(CameraError::Unsupported)
    }

    async fn stop_recording(&mut self) -> Result<VideoData, CameraError> {
        Err(CameraError::Unsupported)
    }

    async fn stop_stream(&mut self) -> Result<(), CameraError> {
        Err(CameraError::Unsupported)
    }

    fn capabilities(&self) -> &CameraCapabilities {
        &self.capabilities
    }

    async fn close(self) -> Result<(), CameraError>
    where
        Self: Sized,
    {
        Ok(())
    }
}
