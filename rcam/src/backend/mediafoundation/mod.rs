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
//! WinRT COM objects in windows-rs do not implement `Send` even for agile
//! (free-threaded) classes. We use `SendWrapper<T>` to assert thread safety
//! where the underlying WinRT type implements `IAgileObject`. All blocking
//! WinRT calls run inside `tokio::task::spawn_blocking` using synchronous
//! spin-wait helpers (`wrt_get` / `wrt_action`) that replace `.await` on
//! WinRT `IAsyncOperation<T>` / `IAsyncAction` values.

pub mod capture;

use std::path::PathBuf;
use std::sync::Mutex;

use async_trait::async_trait;
use windows::core::{Interface, HSTRING};
use windows::Foundation::{AsyncStatus, IAsyncAction, IAsyncInfo};
use windows::Media::Capture::{
    LowLagMediaRecording, LowLagPhotoCapture, MediaCapture, MediaCaptureInitializationSettings,
};
use windows::Media::MediaProperties::{
    ImageEncodingProperties, MediaEncodingProfile, VideoEncodingQuality,
};
use windows::Storage::Streams::{DataReader, IInputStream, IRandomAccessStream};

use crate::traits::CameraDevice;
use crate::{
    CameraCapabilities, CameraConfig, CameraError, CameraInfo, Frame, FrameFormat, RecordingOutput,
    Resolution, VideoData, VideoOutput,
};

use capture::wrt_get;

// ---------------------------------------------------------------------------
// SendWrapper — thin newtype that asserts T is safe to send across threads
// ---------------------------------------------------------------------------

/// Marks a WinRT COM object as `Send + Sync`.
///
/// SAFETY: The wrapped type must implement `IAgileObject` (WinRT free-threaded
/// marshal), guaranteeing safe use from any thread. All mutable state is
/// additionally protected by the `Mutex` in `MfCamera`.
struct SendWrapper<T>(T);
unsafe impl<T> Send for SendWrapper<T> {}
unsafe impl<T> Sync for SendWrapper<T> {}

impl<T: Clone> Clone for SendWrapper<T> {
    fn clone(&self) -> Self {
        SendWrapper(self.0.clone())
    }
}

impl<T> std::ops::Deref for SendWrapper<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}

// ---------------------------------------------------------------------------
// Synchronous WinRT async helpers
// ---------------------------------------------------------------------------

fn wrt_action(op: IAsyncAction) -> windows::core::Result<()> {
    loop {
        let status = op.Status()?;
        if status == AsyncStatus::Completed {
            return Ok(());
        }
        if status != AsyncStatus::Started {
            return Err(windows::core::Error::from(op.ErrorCode()?));
        }
        std::hint::spin_loop();
    }
}

/// Wait on any WinRT async operation that exposes `IAsyncInfo` but is not
/// an `IAsyncOperation<T>` (e.g. `DataReaderLoadOperation`).
fn wrt_wait<T: Interface>(op: &T) -> windows::core::Result<()> {
    let info: IAsyncInfo = op.cast()?;
    loop {
        let status = info.Status()?;
        if status == AsyncStatus::Completed {
            return Ok(());
        }
        if status != AsyncStatus::Started {
            return Err(windows::core::Error::from(info.ErrorCode()?));
        }
        std::hint::spin_loop();
    }
}

// ---------------------------------------------------------------------------
// MfCamera
// ---------------------------------------------------------------------------

/// Windows Media Foundation camera handle.
pub struct MfCamera {
    /// Initialised `MediaCapture` instance (wrapped for cross-thread safety).
    capture: SendWrapper<MediaCapture>,
    /// Cached resolution from config, used when building `Frame` metadata.
    resolution: Resolution,
    /// Active `LowLagMediaRecording` session while recording.
    recording: Mutex<Option<ActiveRecording>>,
    capabilities: CameraCapabilities,
}

struct ActiveRecording {
    /// Session wrapped for cross-thread safety (passed into spawn_blocking).
    session: SendWrapper<LowLagMediaRecording>,
    output_path: PathBuf,
    is_temp: bool,
}

// SAFETY: MfCamera's WinRT objects implement IAgileObject; Mutex guards
// mutable recording state.
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
                capture: SendWrapper(capture),
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
        // Clone the SendWrapper<MediaCapture> (Send) so it can cross thread boundary.
        let capture = self.capture.clone();
        let resolution = self.resolution.clone();

        tokio::task::spawn_blocking(move || {
            // Encode to JPEG in-memory.
            let props = ImageEncodingProperties::CreateJpeg()
                .map_err(|e| CameraError::Backend(e.to_string()))?;

            // Deref SendWrapper<MediaCapture> → &MediaCapture for method calls.
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

            // CapturedFrame implements IRandomAccessStream and IInputStream.
            // Cast to IRandomAccessStream for Size(), and to IInputStream for DataReader.
            let frame = photo
                .Frame()
                .map_err(|e| CameraError::Backend(e.to_string()))?;

            let ras: IRandomAccessStream = frame
                .cast()
                .map_err(|e| CameraError::Backend(e.to_string()))?;
            let size = ras
                .Size()
                .map_err(|e| CameraError::Backend(e.to_string()))? as u32;

            let input: IInputStream = frame
                .cast()
                .map_err(|e| CameraError::Backend(e.to_string()))?;
            let reader = DataReader::CreateDataReader(&input)
                .map_err(|e| CameraError::Backend(e.to_string()))?;

            // LoadAsync returns DataReaderLoadOperation (not IAsyncOperation<T>),
            // so use the IAsyncInfo-based wrt_wait helper.
            let load_op = reader
                .LoadAsync(size)
                .map_err(|e| CameraError::Backend(e.to_string()))?;
            wrt_wait(&load_op).map_err(|e| CameraError::Backend(e.to_string()))?;

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

        // Clone the SendWrapper<MediaCapture> so it can cross the thread boundary.
        let capture = self.capture.clone();
        let path_clone = path.clone();

        let session = tokio::task::spawn_blocking(move || {
            let profile = MediaEncodingProfile::CreateMp4(VideoEncodingQuality::Auto)
                .map_err(|e| CameraError::Backend(e.to_string()))?;

            let output_file = open_or_create_storage_file_sync(&path_clone)?;

            // Deref SendWrapper<MediaCapture> → &MediaCapture for the WinRT call.
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

            // Wrap in SendWrapper so the value can cross the spawn_blocking boundary.
            Ok::<SendWrapper<LowLagMediaRecording>, CameraError>(SendWrapper(session))
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

        // session is SendWrapper<LowLagMediaRecording> (Send), safe to move into closure.
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
