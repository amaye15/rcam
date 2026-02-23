//! AMediaRecorder-based video recording — Phase 5 implementation.
//!
//! `AndroidRecorder` wraps `AMediaRecorder` (NDK API 26+) and records MP4 /
//! H.264 video via a camera surface input.  The caller is responsible for:
//!
//! 1. Calling `AndroidRecorder::new()` to create and configure the recorder.
//! 2. Passing `input_surface()` to the camera session as an additional
//!    output (via `Camera2Session::add_output()`).
//! 3. Calling `start()` after the surface is wired into the session.
//! 4. Calling `stop()` when done, then removing the surface from the session.
//!
//! Requires Android API 26+ for `AMediaRecorder_getInputSurface`.

use std::ffi::CString;
use std::path::PathBuf;

use rcam_sys_android as ffi;

use crate::{CameraError, Resolution};

/// Active `AMediaRecorder` session.
pub(super) struct AndroidRecorder {
    recorder:      *mut ffi::AMediaRecorder,
    /// Surface owned by the recorder — caller must NOT free it separately.
    input_surface: *mut ffi::ANativeWindow,
    pub output_path: PathBuf,
    pub is_temp:     bool,
}

// SAFETY: AMediaRecorder is internally thread-safe.
unsafe impl Send for AndroidRecorder {}
unsafe impl Sync for AndroidRecorder {}

impl AndroidRecorder {
    /// Create and configure a new `AMediaRecorder` for MP4 / H.264.
    ///
    /// After construction the recorder is prepared but NOT started — call
    /// `start()` once the surface is connected to the capture session.
    pub fn new(
        output_path: PathBuf,
        is_temp: bool,
        resolution: &Resolution,
        frame_rate: u32,
    ) -> Result<Self, CameraError> {
        let recorder = unsafe { ffi::AMediaRecorder_new() };
        if recorder.is_null() {
            return Err(CameraError::Backend("AMediaRecorder_new returned null".into()));
        }

        // Helper that aborts on NDK error.
        macro_rules! media_ok {
            ($expr:expr, $msg:literal) => {{
                let status = unsafe { $expr };
                if status != ffi::AMEDIA_OK {
                    unsafe { ffi::AMediaRecorder_delete(recorder) };
                    return Err(CameraError::Backend(format!(
                        "{}: media status {}",
                        $msg, status
                    )));
                }
            }};
        }

        media_ok!(
            ffi::AMediaRecorder_setVideoSource(recorder, ffi::AMEDIARECORDER_VIDEO_SOURCE_SURFACE),
            "setVideoSource"
        );
        media_ok!(
            ffi::AMediaRecorder_setOutputFormat(recorder, ffi::AMEDIARECORDER_OUTPUT_FORMAT_MPEG_4),
            "setOutputFormat"
        );
        media_ok!(
            ffi::AMediaRecorder_setVideoEncoder(recorder, ffi::AMEDIARECORDER_VIDEO_ENCODER_H264),
            "setVideoEncoder"
        );
        media_ok!(
            ffi::AMediaRecorder_setVideoSize(
                recorder,
                resolution.width as std::os::raw::c_int,
                resolution.height as std::os::raw::c_int,
            ),
            "setVideoSize"
        );
        media_ok!(
            ffi::AMediaRecorder_setVideoFrameRate(recorder, frame_rate as std::os::raw::c_int),
            "setVideoFrameRate"
        );
        // ~8 Mbps is a reasonable target for 720p+ H.264.
        media_ok!(
            ffi::AMediaRecorder_setVideoEncodingBitRate(recorder, 8_000_000),
            "setVideoEncodingBitRate"
        );

        let path_c = CString::new(output_path.to_string_lossy().as_bytes())
            .map_err(|_| CameraError::Backend("invalid output path".into()))?;
        media_ok!(
            ffi::AMediaRecorder_setOutputFilePath(recorder, path_c.as_ptr()),
            "setOutputFilePath"
        );

        media_ok!(ffi::AMediaRecorder_prepare(recorder), "AMediaRecorder_prepare");

        // Retrieve the input surface (valid for the recorder's lifetime).
        let input_surface = unsafe { ffi::AMediaRecorder_getInputSurface(recorder) };
        if input_surface.is_null() {
            unsafe { ffi::AMediaRecorder_delete(recorder) };
            return Err(CameraError::Backend(
                "AMediaRecorder_getInputSurface returned null — Android API 26+ required".into(),
            ));
        }

        Ok(Self { recorder, input_surface, output_path, is_temp })
    }

    /// The `ANativeWindow*` surface to register as a camera capture target.
    /// **Do not free this pointer** — it is owned by the `AMediaRecorder`.
    pub fn input_surface(&self) -> *mut ffi::ANativeWindow {
        self.input_surface
    }

    /// Begin encoding frames from the camera surface.
    /// Call this after `Camera2Session::add_output()` returns successfully.
    pub fn start(&self) -> Result<(), CameraError> {
        let status = unsafe { ffi::AMediaRecorder_start(self.recorder) };
        if status != ffi::AMEDIA_OK {
            Err(CameraError::Backend(format!("AMediaRecorder_start: status {}", status)))
        } else {
            Ok(())
        }
    }

    /// Stop encoding, finalise the MP4, and return the output file path.
    ///
    /// Consumes `self` — the recorder is deleted after this call.
    pub fn stop(mut self) -> Result<PathBuf, CameraError> {
        let status = unsafe { ffi::AMediaRecorder_stop(self.recorder) };
        if status != ffi::AMEDIA_OK {
            // Leak the recorder pointer to avoid a double-free in Drop
            // (we call delete here on error, then set to null).
            unsafe { ffi::AMediaRecorder_delete(self.recorder) };
            self.recorder = std::ptr::null_mut();
            return Err(CameraError::Backend(format!(
                "AMediaRecorder_stop: status {}",
                status
            )));
        }
        unsafe { ffi::AMediaRecorder_delete(self.recorder) };
        self.recorder = std::ptr::null_mut();
        Ok(self.output_path.clone())
    }
}

impl Drop for AndroidRecorder {
    fn drop(&mut self) {
        if !self.recorder.is_null() {
            unsafe { ffi::AMediaRecorder_delete(self.recorder) };
        }
    }
}
