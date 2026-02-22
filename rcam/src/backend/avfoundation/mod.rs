//! AVFoundation backend — iOS and macOS.
//!
//! Phase 1: stub implementation. Phase 2 will wire up the real AVFoundation
//! Objective-C APIs via `objc2-av-foundation`.

pub mod device;
pub mod recorder;
pub mod session;

use async_trait::async_trait;

use crate::{
    CameraCapabilities, CameraConfig, CameraError, CameraInfo, Frame, RecordingOutput, VideoData,
};
use crate::traits::CameraDevice;

/// AVFoundation camera handle.
pub struct AvfCamera {
    _session: session::AvfSession,
    capabilities: CameraCapabilities,
}

#[async_trait]
impl CameraDevice for AvfCamera {
    async fn enumerate() -> Result<Vec<CameraInfo>, CameraError>
    where
        Self: Sized,
    {
        device::enumerate_devices()
    }

    async fn open(config: CameraConfig) -> Result<Self, CameraError>
    where
        Self: Sized,
    {
        let _session = session::AvfSession::new(&config)?;
        let capabilities = CameraCapabilities {
            supported_resolutions: vec![],
            supported_frame_rates: vec![],
            supported_formats: vec![],
            has_torch: false,
            has_zoom: false,
        };
        Ok(Self { _session, capabilities })
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
