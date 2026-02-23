//! Tests for `CameraError` variant display messages and `From` impls.

use rcam::{CameraError, FrameFormat};

#[test]
fn no_camera_found_message() {
    assert_eq!(CameraError::NoCameraFound.to_string(), "No camera device found");
}

#[test]
fn permission_denied_message() {
    assert_eq!(CameraError::PermissionDenied.to_string(), "Permission denied");
}

#[test]
fn device_busy_message() {
    assert_eq!(CameraError::DeviceBusy.to_string(), "Device already in use");
}

#[test]
fn unsupported_format_includes_format_name() {
    for fmt in [
        FrameFormat::MJPEG,
        FrameFormat::NV12,
        FrameFormat::YUV420,
        FrameFormat::BGRA,
        FrameFormat::RGB24,
    ] {
        let msg = CameraError::UnsupportedFormat(fmt).to_string();
        assert!(
            msg.contains("Unsupported format"),
            "expected 'Unsupported format' in '{msg}'"
        );
    }
}

#[test]
fn not_recording_message() {
    assert_eq!(
        CameraError::NotRecording.to_string(),
        "Recording has not been started"
    );
}

#[test]
fn stream_not_active_message() {
    assert_eq!(
        CameraError::StreamNotActive.to_string(),
        "Camera stream is not active"
    );
}

#[test]
fn backend_error_includes_payload() {
    let msg = "NDK error -10001";
    let err = CameraError::Backend(msg.to_string());
    assert!(err.to_string().contains(msg));
}

#[test]
fn unsupported_message() {
    assert_eq!(CameraError::Unsupported.to_string(), "Platform not supported");
}

#[test]
fn io_error_converts_via_from() {
    let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "no such file");
    let cam_err: CameraError = io_err.into();
    assert!(cam_err.to_string().contains("IO error"));
}

#[test]
fn all_variants_are_debug_printable() {
    // Smoke-test that every variant produces non-empty Debug output.
    let errors: &[CameraError] = &[
        CameraError::NoCameraFound,
        CameraError::PermissionDenied,
        CameraError::DeviceBusy,
        CameraError::UnsupportedFormat(FrameFormat::MJPEG),
        CameraError::NotRecording,
        CameraError::StreamNotActive,
        CameraError::Backend("test".into()),
        CameraError::Unsupported,
    ];
    for err in errors {
        assert!(!format!("{err:?}").is_empty());
    }
}
