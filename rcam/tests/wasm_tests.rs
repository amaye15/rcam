//! Pure-logic tests that run inside a headless browser via `wasm-pack test`.
//!
//! These tests do NOT call camera APIs (which require user permission) but
//! validate that core types and error messages compile and behave correctly
//! in a WASM environment.
//!
//! Run with:
//! ```
//! wasm-pack test --headless --chrome rcam/
//! ```

#![cfg(target_arch = "wasm32")]

use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

use rcam::{CameraError, CameraPosition, FrameFormat, Resolution};

// ---------------------------------------------------------------------------
// Resolution
// ---------------------------------------------------------------------------

#[wasm_bindgen_test]
fn resolution_fields_accessible() {
    let r = Resolution {
        width: 1280,
        height: 720,
    };
    assert_eq!(r.width, 1280);
    assert_eq!(r.height, 720);
}

#[wasm_bindgen_test]
fn resolution_equality() {
    let a = Resolution {
        width: 640,
        height: 480,
    };
    let b = Resolution {
        width: 640,
        height: 480,
    };
    assert_eq!(a, b);
}

// ---------------------------------------------------------------------------
// CameraPosition
// ---------------------------------------------------------------------------

#[wasm_bindgen_test]
fn camera_position_variants_exist() {
    let _ = CameraPosition::Front;
    let _ = CameraPosition::Back;
    let _ = CameraPosition::External;
    let _ = CameraPosition::Unknown;
}

// ---------------------------------------------------------------------------
// FrameFormat
// ---------------------------------------------------------------------------

#[wasm_bindgen_test]
fn frame_format_variants_exist() {
    let _ = FrameFormat::MJPEG;
    let _ = FrameFormat::NV12;
    let _ = FrameFormat::YUV420;
    let _ = FrameFormat::BGRA;
    let _ = FrameFormat::RGB24;
}

// ---------------------------------------------------------------------------
// CameraError
// ---------------------------------------------------------------------------

#[wasm_bindgen_test]
fn error_no_camera_found_message_on_wasm() {
    let msg = CameraError::NoCameraFound.to_string();
    assert!(!msg.is_empty());
    assert!(msg.to_lowercase().contains("camera"));
}

#[wasm_bindgen_test]
fn error_backend_preserves_payload() {
    let payload = "NDK -10001";
    let err = CameraError::Backend(payload.to_string());
    assert!(err.to_string().contains(payload));
}

#[wasm_bindgen_test]
fn error_unsupported_message_on_wasm() {
    let msg = CameraError::Unsupported.to_string();
    assert!(!msg.is_empty());
}
