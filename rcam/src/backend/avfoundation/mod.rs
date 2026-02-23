//! AVFoundation backend — macOS / iOS — Phase 2 implementation.

pub mod device;
pub mod recorder;
pub mod session;

use std::sync::Mutex;

use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::traits::CameraDevice;
use crate::{
    CameraCapabilities, CameraConfig, CameraError, CameraInfo, Frame, RecordingOutput, VideoData,
    VideoOutput,
};

use recorder::AvfRecorder;
use session::AvfSession;

// ---------------------------------------------------------------------------
// AvfCamera — the public camera handle
// ---------------------------------------------------------------------------

/// AVFoundation camera handle for macOS / iOS.
pub struct AvfCamera {
    /// The live capture session; accessed through a std Mutex for sync calls.
    session: Mutex<AvfSession>,
    /// Decoded frames from the FrameDelegate; async-awaitable separately.
    frame_rx: tokio::sync::Mutex<mpsc::UnboundedReceiver<Frame>>,
    /// Active recording, if any.
    recording: Mutex<Option<AvfRecorder>>,
    capabilities: CameraCapabilities,
}

// SAFETY: `AvfCamera` serialises all ObjC calls through Mutex guards.
// The ObjC objects themselves are thread-safe via atomic retain/release.
unsafe impl Send for AvfCamera {}
unsafe impl Sync for AvfCamera {}

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
        // Ensure the user has granted camera access before touching any device.
        tokio::task::spawn_blocking(device::request_permission)
            .await
            .map_err(|e| CameraError::Backend(e.to_string()))??;

        let avf_device = if let Some(id) = &config.device_id {
            device::device_with_id(id).ok_or(CameraError::NoCameraFound)?
        } else {
            // Honour the requested position (Front / Back / Unknown → default).
            device::device_for_position(config.position).ok_or(CameraError::NoCameraFound)?
        };

        let (avf_session, frame_rx) = AvfSession::new(&config, avf_device)?;

        let capabilities = CameraCapabilities {
            supported_resolutions: vec![],
            supported_frame_rates: vec![],
            supported_formats: vec![],
            has_torch: false,
            has_zoom: false,
        };

        Ok(Self {
            session: Mutex::new(avf_session),
            frame_rx: tokio::sync::Mutex::new(frame_rx),
            recording: Mutex::new(None),
            capabilities,
        })
    }

    async fn start_stream(&mut self) -> Result<(), CameraError> {
        // The session is already running after `open()`; nothing extra needed.
        Ok(())
    }

    async fn capture_frame(&self) -> Result<Frame, CameraError> {
        self.frame_rx
            .lock()
            .await
            .recv()
            .await
            .ok_or(CameraError::StreamNotActive)
    }

    async fn take_photo(&self) -> Result<Frame, CameraError> {
        let mut rx = self.frame_rx.lock().await;
        // Skip frames that are completely black — these occur during camera
        // auto-exposure warmup (typically the first 0–1 s after open()).
        // Sample the first 256 pixels; any average luma > 1.0 means real content.
        // We cap at 30 frames (~1 s at 30 fps) so a legitimately dark scene
        // eventually returns rather than waiting forever.
        for _ in 0..30 {
            let frame = rx.recv().await.ok_or(CameraError::StreamNotActive)?;
            let avg_lum: f32 = frame
                .data
                .chunks_exact(4)
                .take(256)
                .map(|p| 0.299 * p[2] as f32 + 0.587 * p[1] as f32 + 0.114 * p[0] as f32)
                .sum::<f32>()
                / 256.0;
            if avg_lum > 1.0 {
                return Ok(frame);
            }
        }
        // Fallback: return the next frame regardless of brightness.
        rx.recv().await.ok_or(CameraError::StreamNotActive)
    }

    async fn start_recording(&mut self, output: RecordingOutput) -> Result<(), CameraError> {
        let (path, is_temp) = match output {
            RecordingOutput::File(p) => (p, false),
            RecordingOutput::Buffer => {
                let mut p = std::env::temp_dir();
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_micros())
                    .unwrap_or(0);
                p.push(format!("rcam_{ts}.mov"));
                (p, true)
            }
        };

        let recorder = {
            let guard = self.session.lock().unwrap();
            AvfRecorder::start(&guard.session, path, is_temp)?
        };

        *self.recording.lock().unwrap() = Some(recorder);
        Ok(())
    }

    async fn stop_recording(&mut self) -> Result<VideoData, CameraError> {
        let recorder = self
            .recording
            .lock()
            .unwrap()
            .take()
            .ok_or(CameraError::NotRecording)?;

        // Signal AVFoundation to flush and close the output file.
        recorder.stop();

        // Destructure so we can await `done_rx` while still accessing
        // `movie_output` and `_delegate` stays alive until end of scope
        // (the delegate must not be dropped before it fires).
        let recorder::AvfRecorder {
            movie_output,
            delegate: _delegate,
            done_rx,
            output_path,
            is_temp,
        } = recorder;

        // Wait for the delegate callback confirming the file is fully written.
        done_rx
            .await
            .map_err(|_| CameraError::Backend("Recording delegate was dropped".into()))?
            .map_err(CameraError::Backend)?;

        // Detach the movie output from the capture session.
        {
            let guard = self.session.lock().unwrap();
            unsafe { guard.session.removeOutput(&movie_output) };
        }

        if is_temp {
            let data =
                std::fs::read(&output_path).map_err(|e| CameraError::Backend(e.to_string()))?;
            std::fs::remove_file(&output_path).ok();
            Ok(VideoData {
                kind: VideoOutput::Buffer(data),
            })
        } else {
            Ok(VideoData {
                kind: VideoOutput::File(output_path),
            })
        }
    }

    async fn stop_stream(&mut self) -> Result<(), CameraError> {
        // Drain any remaining buffered frames.
        let mut rx = self.frame_rx.lock().await;
        while rx.try_recv().is_ok() {}
        Ok(())
    }

    fn capabilities(&self) -> &CameraCapabilities {
        &self.capabilities
    }

    async fn close(self) -> Result<(), CameraError>
    where
        Self: Sized,
    {
        let Self {
            session,
            frame_rx: _,
            recording,
            capabilities: _,
        } = self;

        // Stop any active recording gracefully.
        if let Some(rec) = recording.into_inner().unwrap() {
            rec.stop();
            let recorder::AvfRecorder {
                movie_output,
                delegate: _delegate,
                done_rx,
                output_path: _,
                is_temp: _,
            } = rec;
            let _ = done_rx.await;
            if let Ok(guard) = session.lock() {
                unsafe { guard.session.removeOutput(&movie_output) };
            }
        }

        // Stop the capture session.
        if let Ok(avf_session) = session.into_inner() {
            avf_session.stop();
        }

        Ok(())
    }
}
