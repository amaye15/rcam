//! Android Camera2 NDK backend — Phase 5 implementation.
//!
//! Uses `rcam-sys-android` FFI bindings to the Camera2 NDK (API 24+) for
//! frame streaming, and `AMediaRecorder` (API 26+) for video recording.
//!
//! # Architecture
//!
//! `AndroidCamera` holds:
//! - A `Camera2Session` that owns the full NDK camera pipeline.
//! - An unbounded Tokio channel for async frame delivery.
//! - An `Option<ActiveRecording>` for in-progress video recordings.
//!
//! All NDK calls are `unsafe`; safety is ensured by serialising access to the
//! camera session through a `std::sync::Mutex`.
//!
//! # Permissions
//!
//! `android.permission.CAMERA` (and `RECORD_AUDIO` for audio) must be granted
//! at the Java layer before calling any of these functions.  Add to your
//! `AndroidManifest.xml`:
//! ```xml
//! <uses-permission android:name="android.permission.CAMERA" />
//! <uses-permission android:name="android.permission.RECORD_AUDIO" />
//! ```

pub mod camera2;
pub mod media;

use std::sync::Mutex;

use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::traits::CameraDevice;
use crate::{
    CameraCapabilities, CameraConfig, CameraError, CameraInfo, Frame, RecordingOutput,
    Resolution, VideoData, VideoOutput,
};

use camera2::Camera2Session;
use media::AndroidRecorder;

// ---------------------------------------------------------------------------
// ActiveRecording
// ---------------------------------------------------------------------------

struct ActiveRecording {
    recorder:       AndroidRecorder,
    /// `ACaptureSessionOutput*` for the recorder surface (opaque pointer).
    extra_output:   *mut rcam_sys_android::ACaptureSessionOutput,
    /// `ACameraOutputTarget*` added to the capture request.
    extra_target:   *mut rcam_sys_android::ACameraOutputTarget,
}

// SAFETY: raw pointers are only accessed while holding the camera session mutex.
unsafe impl Send for ActiveRecording {}

// ---------------------------------------------------------------------------
// AndroidCamera
// ---------------------------------------------------------------------------

/// Android Camera2 NDK camera handle.
pub struct AndroidCamera {
    /// The live capture pipeline; guards all NDK calls.
    session: Mutex<Camera2Session>,
    /// Async-awaitable frame queue, produced by the NDK image callback.
    frame_rx: tokio::sync::Mutex<mpsc::UnboundedReceiver<Frame>>,
    /// Active recording session, if any.
    recording: Mutex<Option<ActiveRecording>>,
    capabilities: CameraCapabilities,
    config: CameraConfig,
}

// SAFETY: `Camera2Session` is `Send + Sync`; `AndroidCamera` serialises all
// mutable access through its `Mutex` fields.
unsafe impl Send for AndroidCamera {}
unsafe impl Sync for AndroidCamera {}

#[async_trait]
impl CameraDevice for AndroidCamera {
    // -----------------------------------------------------------------------
    // Enumeration
    // -----------------------------------------------------------------------

    async fn enumerate() -> Result<Vec<CameraInfo>, CameraError>
    where
        Self: Sized,
    {
        // Enumerate involves sync NDK I/O; run on the blocking thread pool.
        tokio::task::spawn_blocking(camera2::enumerate_devices)
            .await
            .map_err(|e| CameraError::Backend(e.to_string()))?
    }

    // -----------------------------------------------------------------------
    // Open
    // -----------------------------------------------------------------------

    async fn open(config: CameraConfig) -> Result<Self, CameraError>
    where
        Self: Sized,
    {
        let (frame_tx, frame_rx) = mpsc::unbounded_channel::<Frame>();

        let device_id = config.device_id.clone();
        let position  = config.position;
        let resolution = config.resolution;
        let frame_rate = config.frame_rate;
        let tx_clone = frame_tx.clone();

        let session = tokio::task::spawn_blocking(move || {
            Camera2Session::open(
                device_id.as_deref(),
                position,
                resolution,
                frame_rate,
                tx_clone,
            )
        })
        .await
        .map_err(|e| CameraError::Backend(e.to_string()))??;

        let capabilities = CameraCapabilities {
            supported_resolutions: vec![],
            supported_frame_rates: vec![],
            supported_formats:     vec![],
            has_torch: false,
            has_zoom:  false,
        };

        Ok(Self {
            session:      Mutex::new(session),
            frame_rx:     tokio::sync::Mutex::new(frame_rx),
            recording:    Mutex::new(None),
            capabilities,
            config,
        })
    }

    // -----------------------------------------------------------------------
    // Streaming
    // -----------------------------------------------------------------------

    async fn start_stream(&mut self) -> Result<(), CameraError> {
        // The capture session is already running after `open()`.
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
        // Skip the first few frames to let auto-exposure settle.
        for _ in 0..10usize {
            let frame = rx.recv().await.ok_or(CameraError::StreamNotActive)?;
            let avg_lum: f32 = frame
                .data
                .iter()
                .take(256)
                .map(|&b| b as f32)
                .sum::<f32>()
                / 256.0;
            if avg_lum > 1.0 {
                return Ok(frame);
            }
        }
        rx.recv().await.ok_or(CameraError::StreamNotActive)
    }

    // -----------------------------------------------------------------------
    // Recording
    // -----------------------------------------------------------------------

    async fn start_recording(&mut self, output: RecordingOutput) -> Result<(), CameraError> {
        if self.recording.lock().unwrap().is_some() {
            return Err(CameraError::Backend("recording already in progress".into()));
        }

        let (output_path, is_temp) = match output {
            RecordingOutput::File(p) => (p, false),
            RecordingOutput::Buffer => {
                let mut p = std::env::temp_dir();
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_micros())
                    .unwrap_or(0);
                p.push(format!("rcam_{ts}.mp4"));
                (p, true)
            }
        };

        let resolution = self.config.resolution;
        let frame_rate = self.config.frame_rate;

        // Create and prepare the recorder on the blocking thread pool.
        let recorder = tokio::task::spawn_blocking(move || {
            AndroidRecorder::new(output_path, is_temp, &resolution, frame_rate)
        })
        .await
        .map_err(|e| CameraError::Backend(e.to_string()))??;

        let recorder_surface = recorder.input_surface();

        // Wire the recorder surface into the camera session.
        let (extra_output, extra_target) = {
            let mut guard = self.session.lock().unwrap();
            guard.add_output(recorder_surface)?
        };

        // Now start the encoder.
        recorder.start()?;

        *self.recording.lock().unwrap() = Some(ActiveRecording {
            recorder,
            extra_output,
            extra_target,
        });

        Ok(())
    }

    async fn stop_recording(&mut self) -> Result<VideoData, CameraError> {
        let active = self
            .recording
            .lock()
            .unwrap()
            .take()
            .ok_or(CameraError::NotRecording)?;

        let ActiveRecording { recorder, extra_output, extra_target } = active;

        let recorder_surface = recorder.input_surface();

        // Stop the encoder and finalise the file.
        let output_path = recorder.stop()?;

        // Remove the recorder surface from the capture session.
        {
            let mut guard = self.session.lock().unwrap();
            guard.remove_output(extra_output, extra_target, recorder_surface)?;
        }

        if {
            // Re-read is_temp from path — we don't store it separately here.
            // We infer by checking if the path is inside temp_dir.
            let tmp = std::env::temp_dir();
            output_path.starts_with(&tmp)
        } {
            let data = std::fs::read(&output_path)
                .map_err(|e| CameraError::Backend(e.to_string()))?;
            std::fs::remove_file(&output_path).ok();
            Ok(VideoData { kind: VideoOutput::Buffer(data) })
        } else {
            Ok(VideoData { kind: VideoOutput::File(output_path) })
        }
    }

    // -----------------------------------------------------------------------
    // Misc
    // -----------------------------------------------------------------------

    async fn stop_stream(&mut self) -> Result<(), CameraError> {
        // Drain buffered frames.
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
        // Stop any active recording gracefully.
        if let Some(active) = self.recording.into_inner().unwrap() {
            let recorder_surface = active.recorder.input_surface();
            let _ = active.recorder.stop();
            if let Ok(mut guard) = self.session.lock() {
                let _ = guard.remove_output(
                    active.extra_output,
                    active.extra_target,
                    recorder_surface,
                );
            }
        }

        // `Camera2Session` stops the repeating request and tears down all NDK
        // objects when it is dropped (called here by the Mutex destruction).
        drop(self.session);
        Ok(())
    }
}
