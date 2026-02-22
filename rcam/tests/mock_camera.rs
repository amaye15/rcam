//! Integration tests using a pure-software mock camera.
//!
//! The mock backend synthesises frames and satisfies the full `CameraDevice`
//! trait without touching any hardware. This lets CI validate the public API
//! on every platform (including Android / iOS runners without a physical camera).

use std::path::PathBuf;

use async_trait::async_trait;
use rcam::{
    CameraCapabilities, CameraConfig, CameraDevice, CameraError, CameraInfo, CameraPosition,
    Frame, FrameFormat, RecordingOutput, Resolution, VideoData, VideoOutput,
};

// ---------------------------------------------------------------------------
// Mock implementation
// ---------------------------------------------------------------------------

struct MockCamera {
    capabilities: CameraCapabilities,
    streaming: bool,
    recording: bool,
}

impl MockCamera {
    fn new() -> Self {
        Self {
            capabilities: CameraCapabilities {
                supported_resolutions: vec![
                    Resolution { width: 640, height: 480 },
                    Resolution { width: 1280, height: 720 },
                ],
                supported_frame_rates: vec![15, 30, 60],
                supported_formats: vec![FrameFormat::BGRA, FrameFormat::NV12],
                has_torch: false,
                has_zoom: false,
            },
            streaming: false,
            recording: false,
        }
    }

    /// Synthesise a 640×480 BGRA checkerboard frame.
    fn synth_frame() -> Frame {
        let width: u32 = 640;
        let height: u32 = 480;
        let mut data = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                let bright = ((x / 32) + (y / 32)) % 2 == 0;
                let luma: u8 = if bright { 220 } else { 40 };
                data.extend_from_slice(&[luma, luma, luma, 255]);
            }
        }
        Frame { data, width, height, format: FrameFormat::BGRA, timestamp_us: 0 }
    }
}

#[async_trait]
impl CameraDevice for MockCamera {
    async fn enumerate() -> Result<Vec<CameraInfo>, CameraError>
    where
        Self: Sized,
    {
        Ok(vec![
            CameraInfo {
                id: "mock-0".to_string(),
                name: "Mock Front Camera".to_string(),
                position: CameraPosition::Front,
                is_default: true,
            },
            CameraInfo {
                id: "mock-1".to_string(),
                name: "Mock Back Camera".to_string(),
                position: CameraPosition::Back,
                is_default: false,
            },
        ])
    }

    async fn open(_config: CameraConfig) -> Result<Self, CameraError>
    where
        Self: Sized,
    {
        Ok(MockCamera::new())
    }

    async fn start_stream(&mut self) -> Result<(), CameraError> {
        self.streaming = true;
        Ok(())
    }

    async fn capture_frame(&self) -> Result<Frame, CameraError> {
        if !self.streaming {
            return Err(CameraError::StreamNotActive);
        }
        Ok(MockCamera::synth_frame())
    }

    async fn take_photo(&self) -> Result<Frame, CameraError> {
        Ok(MockCamera::synth_frame())
    }

    async fn start_recording(&mut self, _output: RecordingOutput) -> Result<(), CameraError> {
        self.recording = true;
        Ok(())
    }

    async fn stop_recording(&mut self) -> Result<VideoData, CameraError> {
        if !self.recording {
            return Err(CameraError::NotRecording);
        }
        self.recording = false;
        // Return a minimal synthetic MP4-like buffer.
        Ok(VideoData { kind: VideoOutput::Buffer(vec![0u8; 128]) })
    }

    async fn stop_stream(&mut self) -> Result<(), CameraError> {
        self.streaming = false;
        Ok(())
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_enumerate_returns_two_devices() {
    let devices = MockCamera::enumerate().await.unwrap();
    assert_eq!(devices.len(), 2);
    assert!(devices[0].is_default);
    assert_eq!(devices[0].id, "mock-0");
}

#[tokio::test]
async fn test_open_and_close() {
    let cam = MockCamera::open(CameraConfig::default()).await.unwrap();
    cam.close().await.unwrap();
}

#[tokio::test]
async fn test_stream_lifecycle() {
    let mut cam = MockCamera::open(CameraConfig::default()).await.unwrap();

    // Capturing before start_stream should fail.
    assert!(matches!(cam.capture_frame().await, Err(CameraError::StreamNotActive)));

    cam.start_stream().await.unwrap();
    let frame = cam.capture_frame().await.unwrap();
    assert_eq!(frame.width, 640);
    assert_eq!(frame.height, 480);
    assert_eq!(frame.data.len(), 640 * 480 * 4);

    cam.stop_stream().await.unwrap();
    cam.close().await.unwrap();
}

#[tokio::test]
async fn test_take_photo_without_stream() {
    let cam = MockCamera::open(CameraConfig::default()).await.unwrap();
    let photo = cam.take_photo().await.unwrap();
    assert_eq!(photo.format, FrameFormat::BGRA);
    cam.close().await.unwrap();
}

#[tokio::test]
async fn test_recording_lifecycle() {
    let mut cam = MockCamera::open(CameraConfig::default()).await.unwrap();

    // stop_recording before start_recording should fail.
    assert!(matches!(cam.stop_recording().await, Err(CameraError::NotRecording)));

    cam.start_recording(RecordingOutput::Buffer).await.unwrap();
    let video = cam.stop_recording().await.unwrap();
    assert!(matches!(video.kind, VideoOutput::Buffer(_)));

    cam.close().await.unwrap();
}

#[tokio::test]
async fn test_capabilities() {
    let cam = MockCamera::open(CameraConfig::default()).await.unwrap();
    let caps = cam.capabilities();
    assert!(caps.supported_resolutions.len() >= 2);
    assert!(caps.supported_frame_rates.contains(&30));
    cam.close().await.unwrap();
}

#[tokio::test]
async fn test_file_recording_output() {
    let mut cam = MockCamera::open(CameraConfig::default()).await.unwrap();
    let path = PathBuf::from("/tmp/rcam_test_output.mp4");
    cam.start_recording(RecordingOutput::File(path)).await.unwrap();
    let _ = cam.stop_recording().await.unwrap();
    cam.close().await.unwrap();
}
