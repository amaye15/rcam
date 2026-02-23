//! Video4Linux2 backend — Linux — Phase 2 implementation.
//!
//! `V4l2Camera::open()` negotiates a pixel format (YUYV → BGRA, or MJPEG)
//! and spawns a blocking background task that continuously feeds frames into
//! a tokio channel.  All synchronous V4L2 I/O runs on tokio's blocking thread
//! pool via `spawn_blocking`, keeping the async executor free.
//!
//! Video recording on Linux has no built-in muxing API; it will be added as
//! part of the optional `ffmpeg-encoding` feature in a later phase.

pub mod capture;

use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::traits::CameraDevice;
use crate::{
    CameraCapabilities, CameraConfig, CameraError, CameraInfo, Frame, FrameFormat,
    RecordingOutput, VideoData,
};

use capture::{capture_loop, open_device};

// ---------------------------------------------------------------------------
// V4l2Camera
// ---------------------------------------------------------------------------

/// V4L2 camera handle for Linux.
///
/// Frames are produced by a blocking background task and delivered through an
/// unbounded MPSC channel. The task exits automatically when this handle is
/// dropped.
pub struct V4l2Camera {
    /// Async-accessible frame queue.
    frame_rx: tokio::sync::Mutex<mpsc::UnboundedReceiver<Frame>>,
    /// Outer async task wrapping the blocking capture loop.
    stream_task: tokio::task::JoinHandle<()>,
    capabilities: CameraCapabilities,
}

impl Drop for V4l2Camera {
    fn drop(&mut self) {
        self.stream_task.abort();
    }
}

#[async_trait]
impl CameraDevice for V4l2Camera {
    async fn enumerate() -> Result<Vec<CameraInfo>, CameraError>
    where
        Self: Sized,
    {
        // Device enumeration involves sync I/O (open + ioctl per device).
        tokio::task::spawn_blocking(capture::enumerate_devices)
            .await
            .map_err(|e| CameraError::Backend(e.to_string()))?
    }

    async fn open(config: CameraConfig) -> Result<Self, CameraError>
    where
        Self: Sized,
    {
        // Negotiate format on the blocking thread pool.
        let handle = tokio::task::spawn_blocking(move || open_device(&config))
            .await
            .map_err(|e| CameraError::Backend(e.to_string()))??;

        let (tx, rx) = mpsc::unbounded_channel::<Frame>();

        // Spawn an async task that runs the blocking capture loop. The loop
        // exits naturally when `tx` can no longer deliver frames (i.e. when
        // `rx` / `V4l2Camera` is dropped).
        let stream_task = tokio::task::spawn(async move {
            tokio::task::spawn_blocking(move || capture_loop(handle, tx))
                .await
                .ok();
        });

        Ok(Self {
            frame_rx: tokio::sync::Mutex::new(rx),
            stream_task,
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
        // Streaming starts in `open()`; this method is a no-op.
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
        let mut skipped = 0usize;

        loop {
            let frame = rx.recv().await.ok_or(CameraError::StreamNotActive)?;

            match frame.format {
                // For raw BGRA frames, skip until the auto-exposure settles.
                FrameFormat::BGRA => {
                    let avg_lum: f32 = frame
                        .data
                        .chunks_exact(4)
                        .take(256)
                        .map(|p| {
                            0.299 * p[2] as f32
                                + 0.587 * p[1] as f32
                                + 0.114 * p[0] as f32
                        })
                        .sum::<f32>()
                        / 256.0;
                    if avg_lum > 1.0 || skipped >= 30 {
                        return Ok(frame);
                    }
                }
                // For compressed formats (MJPEG), skip a handful of frames to
                // let auto-exposure settle, then return.
                _ => {
                    if skipped >= 5 {
                        return Ok(frame);
                    }
                }
            }

            skipped += 1;
        }
    }

    async fn start_recording(&mut self, _output: RecordingOutput) -> Result<(), CameraError> {
        // Video recording on Linux requires an external encoder (e.g. ffmpeg).
        // This will be wired up via the optional `ffmpeg-encoding` feature.
        Err(CameraError::Unsupported)
    }

    async fn stop_recording(&mut self) -> Result<VideoData, CameraError> {
        Err(CameraError::Unsupported)
    }

    async fn stop_stream(&mut self) -> Result<(), CameraError> {
        // Drain any buffered frames so subsequent calls to capture_frame
        // block on fresh data rather than stale buffered frames.
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
        let Self { frame_rx, stream_task, capabilities: _ } = self;
        // Dropping the receiver causes `tx.send()` in the capture loop to
        // fail, which cleanly terminates the blocking thread.
        drop(frame_rx);
        // Abort the wrapper async task in case the loop hasn't exited yet.
        stream_task.abort();
        Ok(())
    }
}
