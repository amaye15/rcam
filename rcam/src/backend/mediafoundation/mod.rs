//! Windows Media Foundation backend — Phase 2 implementation.
//!
//! Uses `Windows.Media.Capture.MediaCapture` as the main capture pipeline.
//!
//! | Operation        | WinRT API                        | Phase 2 status |
//! |------------------|----------------------------------|----------------|
//! | Enumeration      | DeviceInformation::FindAllAsync  | ✅ implemented |
//! | Open / init      | MediaCapture::InitializeAsync    | ✅ implemented |
//! | Still photo      | LowLagPhotoCapture               | ✅ implemented |
//! | Video recording  | LowLagMediaRecording             | ✅ implemented |
//! | Frame streaming  | MediaFrameReader                 | 🔜 Phase 3     |
//!
//! Frame streaming (`capture_frame`) returns `Unsupported` for now.
//!
//! # Threading
//!
//! WinRT `IAsyncOperation<T>` does not implement `std::future::Future` in all
//! windows-rs configurations. We resolve async operations synchronously via the
//! `capture::wrt_get` / `wrt_action` spin-wait helpers inside
//! `tokio::task::spawn_blocking`, which avoids blocking the async executor.

pub mod capture;

use std::path::PathBuf;
use std::sync::Mutex;

use async_trait::async_trait;
use windows::core::HSTRING;
use windows::Foundation::{AsyncStatus, IAsyncAction};
use windows::Media::Capture::{
    LowLagMediaRecording, LowLagPhotoCapture, MediaCapture, MediaCaptureInitializationSettings,
};
use windows::Media::MediaProperties::{
    ImageEncodingProperties, MediaEncodingProfile, VideoEncodingQuality,
};
use windows::Storage::Streams::DataReader;

use crate::traits::CameraDevice;
use crate::{
    CameraCapabilities, CameraConfig, CameraError, CameraInfo, Frame, FrameFormat, RecordingOutput,
    Resolution, VideoData, VideoOutput,
};

use capture::wrt_get;

// ---------------------------------------------------------------------------
// Synchronous IAsyncAction helper
// ---------------------------------------------------------------------------

fn wrt_action(op: IAsyncAction) -> windows::core::Result<()> {
    loop {
        let status = op.Status()?;
        if status == AsyncStatus::Completed {
            return Ok(());
        }
        if status != AsyncStatus::Started {
            // Error or Canceled — get error HRESULT from the operation.
            return Err(windows::core::Error::from(op.ErrorCode()?));
        }
        std::hint::spin_loop();
    }
}

// ---------------------------------------------------------------------------
// MfCamera
// ---------------------------------------------------------------------------

/// Windows Media Foundation camera handle.
pub struct MfCamera {
    /// Initialised MediaCapture instance for the chosen device.
    capture: MediaCapture,
    /// Cached resolution from config, used when building `Frame` metadata.
    resolution: Resolution,
    /// Active `LowLagMediaRecording` session while recording, plus whether the
    /// output should be read back into a buffer after finishing.
    recording: Mutex<Option<ActiveRecording>>,
    capabilities: CameraCapabilities,
}

struct ActiveRecording {
    session: LowLagMediaRecording,
    output_path: PathBuf,
    is_temp: bool,
}

// SAFETY: WinRT `MediaCapture` implements `IAgileObject` and is safe to use
// from multiple threads. The `Mutex` guards mutable recording state.
unsafe impl Send for MfCamera {}
unsafe impl Sync for MfCamera {}

#[async_trait]
impl CameraDevice for MfCamera {
    // -----------------------------------------------------------------------
    // Enumeration
    // -----------------------------------------------------------------------

    async fn enumerate() -> Result<Vec<CameraInfo>, CameraError>
    where
        Self: Sized,
    {
        tokio::task::spawn_blocking(capture::enumerate_devices)
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
        tokio::task::spawn_blocking(move || {
            let settings = MediaCaptureInitializationSettings::new()
                .map_err(|e| CameraError::Backend(e.to_string()))?;

            if let Some(id) = &config.device_id {
                settings
                    .SetVideoDeviceId(&HSTRING::from(id.as_str()))
                    .map_err(|e| CameraError::Backend(e.to_string()))?;
            }

            let capture = MediaCapture::new().map_err(|e| CameraError::Backend(e.to_string()))?;

            wrt_action(
                capture
                    .InitializeWithSettingsAsync(&settings)
                    .map_err(|e| CameraError::Backend(e.to_string()))?,
            )
            .map_err(|e| CameraError::Backend(e.to_string()))?;

            Ok(Self {
                capture,
                resolution: config.resolution,
                recording: Mutex::new(None),
                capabilities: CameraCapabilities {
                    supported_resolutions: vec![],
                    supported_frame_rates: vec![],
                    supported_formats: vec![],
                    has_torch: false,
                    has_zoom: false,
                },
            })
        })
        .await
        .map_err(|e| CameraError::Backend(e.to_string()))?
    }

    // -----------------------------------------------------------------------
    // Streaming (frame-by-frame)
    // -----------------------------------------------------------------------

    async fn start_stream(&mut self) -> Result<(), CameraError> {
        // MediaCapture preview starts implicitly; a full MediaFrameReader
        // implementation will be added in Phase 3.
        Ok(())
    }

    async fn capture_frame(&self) -> Result<Frame, CameraError> {
        // Frame streaming requires MediaFrameReader — Phase 3.
        Err(CameraError::Unsupported)
    }

    // -----------------------------------------------------------------------
    // Still photo
    // -----------------------------------------------------------------------

    async fn take_photo(&self) -> Result<Frame, CameraError> {
        let capture = self.capture.clone();
        let resolution = self.resolution.clone();

        tokio::task::spawn_blocking(move || {
            // Encode to JPEG in-memory; most straightforward format to return as
            // raw bytes without requiring an external decoder dependency.
            let props = ImageEncodingProperties::CreateJpeg()
                .map_err(|e| CameraError::Backend(e.to_string()))?;

            let plpc: LowLagPhotoCapture = wrt_get(
                capture
                    .PrepareLowLagPhotoCaptureAsync(&props)
                    .map_err(|e| CameraError::Backend(e.to_string()))?,
            )
            .map_err(|e| CameraError::Backend(e.to_string()))?;

            let photo = wrt_get(
                plpc.CaptureAsync()
                    .map_err(|e| CameraError::Backend(e.to_string()))?,
            )
            .map_err(|e| CameraError::Backend(e.to_string()))?;

            // Finish the capture session so the device returns to idle.
            wrt_action(
                plpc.FinishAsync()
                    .map_err(|e| CameraError::Backend(e.to_string()))?,
            )
            .map_err(|e| CameraError::Backend(e.to_string()))?;

            // Read the JPEG bytes from the captured frame's stream.
            let frame = photo
                .Frame()
                .map_err(|e| CameraError::Backend(e.to_string()))?;
            let stream = frame
                .GetStream()
                .map_err(|e| CameraError::Backend(e.to_string()))?;

            let size = stream
                .Size()
                .map_err(|e| CameraError::Backend(e.to_string()))? as u32;

            let reader = DataReader::CreateDataReader(&stream)
                .map_err(|e| CameraError::Backend(e.to_string()))?;

            wrt_get(
                reader
                    .LoadAsync(size)
                    .map_err(|e| CameraError::Backend(e.to_string()))?,
            )
            .map_err(|e| CameraError::Backend(e.to_string()))?;

            let mut data = vec![0u8; size as usize];
            reader
                .ReadBytes(&mut data)
                .map_err(|e| CameraError::Backend(e.to_string()))?;

            Ok(Frame {
                data,
                width: resolution.width,
                height: resolution.height,
                format: FrameFormat::MJPEG,
                timestamp_us: timestamp_us_now(),
            })
        })
        .await
        .map_err(|e| CameraError::Backend(e.to_string()))?
    }

    // -----------------------------------------------------------------------
    // Recording
    // -----------------------------------------------------------------------

    async fn start_recording(&mut self, output: RecordingOutput) -> Result<(), CameraError> {
        if self.recording.lock().unwrap().is_some() {
            return Err(CameraError::Backend("recording already in progress".into()));
        }

        let (path, is_temp) = match output {
            RecordingOutput::File(p) => (p, false),
            RecordingOutput::Buffer => {
                let mut p = std::env::temp_dir();
                let ts = timestamp_us_now();
                p.push(format!("rcam_{ts}.mp4"));
                (p, true)
            }
        };

        let capture = self.capture.clone();
        let path_clone = path.clone();

        let session = tokio::task::spawn_blocking(move || {
            // Encode to MP4 / H.264 at Auto quality (device picks best).
            let profile = MediaEncodingProfile::CreateMp4(VideoEncodingQuality::Auto)
                .map_err(|e| CameraError::Backend(e.to_string()))?;

            // Open (or create) the output file via the Storage API.
            let output_file = open_or_create_storage_file_sync(&path_clone)?;

            let session: LowLagMediaRecording = wrt_get(
                capture
                    .PrepareLowLagRecordToStorageFileAsync(&profile, &output_file)
                    .map_err(|e| CameraError::Backend(e.to_string()))?,
            )
            .map_err(|e| CameraError::Backend(e.to_string()))?;

            wrt_action(
                session
                    .StartAsync()
                    .map_err(|e| CameraError::Backend(e.to_string()))?,
            )
            .map_err(|e| CameraError::Backend(e.to_string()))?;

            Ok::<LowLagMediaRecording, CameraError>(session)
        })
        .await
        .map_err(|e| CameraError::Backend(e.to_string()))??;

        *self.recording.lock().unwrap() = Some(ActiveRecording {
            session,
            output_path: path,
            is_temp,
        });
        Ok(())
    }

    async fn stop_recording(&mut self) -> Result<VideoData, CameraError> {
        let ActiveRecording {
            session,
            output_path,
            is_temp,
        } = self
            .recording
            .lock()
            .unwrap()
            .take()
            .ok_or(CameraError::NotRecording)?;

        tokio::task::spawn_blocking(move || {
            wrt_action(
                session
                    .StopAsync()
                    .map_err(|e| CameraError::Backend(e.to_string()))?,
            )
            .map_err(|e| CameraError::Backend(e.to_string()))?;

            wrt_action(
                session
                    .FinishAsync()
                    .map_err(|e| CameraError::Backend(e.to_string()))?,
            )
            .map_err(|e| CameraError::Backend(e.to_string()))?;

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
        })
        .await
        .map_err(|e| CameraError::Backend(e.to_string()))?
    }

    // -----------------------------------------------------------------------
    // Misc
    // -----------------------------------------------------------------------

    async fn stop_stream(&mut self) -> Result<(), CameraError> {
        Ok(())
    }

    fn capabilities(&self) -> &CameraCapabilities {
        &self.capabilities
    }

    async fn close(self) -> Result<(), CameraError>
    where
        Self: Sized,
    {
        // If recording was in progress, stop it gracefully (best-effort).
        if let Some(rec) = self.recording.into_inner().unwrap() {
            let _ = rec.session.StopAsync();
            let _ = rec.session.FinishAsync();
        }
        // MediaCapture is released when dropped (COM reference counting).
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Returns the current time as microseconds since the Unix epoch.
fn timestamp_us_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0)
}

/// Create or open a `StorageFile` at the given path for WinRT recording APIs.
///
/// Must be called on a blocking thread; uses [`wrt_get`] internally.
fn open_or_create_storage_file_sync(
    path: &PathBuf,
) -> Result<windows::Storage::StorageFile, CameraError> {
    use windows::Storage::{CreationCollisionOption, StorageFolder};

    let parent = path
        .parent()
        .ok_or_else(|| CameraError::Backend("invalid output path: no parent directory".into()))?;
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| CameraError::Backend("invalid output path: no file name".into()))?;

    let folder = wrt_get(
        StorageFolder::GetFolderFromPathAsync(&HSTRING::from(parent.to_string_lossy().as_ref()))
            .map_err(|e| CameraError::Backend(e.to_string()))?,
    )
    .map_err(|e| CameraError::Backend(e.to_string()))?;

    let file = wrt_get(
        folder
            .CreateFileAsync(
                &HSTRING::from(file_name),
                CreationCollisionOption::ReplaceExisting,
            )
            .map_err(|e| CameraError::Backend(e.to_string()))?,
    )
    .map_err(|e| CameraError::Backend(e.to_string()))?;

    Ok(file)
}
