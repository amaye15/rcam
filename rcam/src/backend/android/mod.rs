//! Android Camera2 NDK backend (Phase 1 stub).
//!
//! Phase 5 will wire up real Camera2 NDK bindings from `rcam-sys-android`.
//! Requires `ANDROID_NDK_ROOT` and API level >= 24.

pub mod camera2;
pub mod media;

use async_trait::async_trait;

use crate::{
    CameraCapabilities, CameraConfig, CameraError, CameraInfo, Frame, RecordingOutput, VideoData,
};
use crate::traits::CameraDevice;

/// Android Camera2 NDK camera handle.
pub struct AndroidCamera {
    capabilities: CameraCapabilities,
}

#[async_trait]
impl CameraDevice for AndroidCamera {
    async fn enumerate() -> Result<Vec<CameraInfo>, CameraError>
    where
        Self: Sized,
    {
        camera2::enumerate_devices()
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
