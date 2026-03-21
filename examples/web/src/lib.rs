//! rcam web demo — thin wasm-bindgen wrapper around the rcam WASM backend.
//!
//! Exports async functions callable from JavaScript:
//! - `enumerate_cameras()` → JS Array of `{id, name}`
//! - `start_camera(device_id?)` → opens the camera + starts the stream
//! - `capture_frame()` → returns RGBA `Uint8Array` (width × height × 4)
//! - `stop_camera()` → stops the stream and releases hardware
//! - `start_recording()` → begins MediaRecorder session
//! - `stop_recording()` → stops and returns raw WebM `Uint8Array`
//!
//! Two sync helpers:
//! - `camera_width()` → u32
//! - `camera_height()` → u32

use std::cell::RefCell;

use rcam::{Camera, CameraConfig, CameraDevice, RecordingOutput, VideoOutput};
use wasm_bindgen::prelude::*;

// ---------------------------------------------------------------------------
// Thread-local state (WASM is single-threaded)
// ---------------------------------------------------------------------------

thread_local! {
    static CAMERA: RefCell<Option<Camera>> = RefCell::new(None);
    static WIDTH:  RefCell<u32>            = RefCell::new(640);
    static HEIGHT: RefCell<u32>            = RefCell::new(480);
}

// ---------------------------------------------------------------------------
// Exported API
// ---------------------------------------------------------------------------

/// List all video-input devices visible to the browser.
///
/// Device labels are empty until `getUserMedia` permission has been granted;
/// call again after `start_camera()` for human-readable names.
#[wasm_bindgen]
pub async fn enumerate_cameras() -> Result<js_sys::Array, JsValue> {
    let cameras = Camera::enumerate()
        .await
        .map_err(|e| JsValue::from_str(&e.to_string()))?;

    let arr = js_sys::Array::new();
    for cam in cameras {
        let obj = js_sys::Object::new();
        js_sys::Reflect::set(&obj, &"id".into(), &JsValue::from_str(&cam.id)).unwrap();
        js_sys::Reflect::set(&obj, &"name".into(), &JsValue::from_str(&cam.name)).unwrap();
        arr.push(&obj);
    }
    Ok(arr)
}

/// Open the camera (triggers browser permission prompt) and start the stream.
///
/// Pass `device_id = null` to use the system default camera.
#[wasm_bindgen]
pub async fn start_camera(device_id: Option<String>) -> Result<(), JsValue> {
    let mut config = CameraConfig::default();
    config.device_id = device_id;

    let mut cam = Camera::open(config)
        .await
        .map_err(|e| JsValue::from_str(&e.to_string()))?;

    cam.start_stream()
        .await
        .map_err(|e| JsValue::from_str(&e.to_string()))?;

    // Store the frame dimensions reported by the backend.
    let caps = cam.capabilities();
    if let Some(res) = caps.supported_resolutions.first() {
        WIDTH.with(|w| *w.borrow_mut() = res.width);
        HEIGHT.with(|h| *h.borrow_mut() = res.height);
    }

    CAMERA.with(|c| *c.borrow_mut() = Some(cam));
    Ok(())
}

/// Capture one frame and return it as an RGBA `Uint8Array`.
///
/// The caller is responsible for rendering the pixels into a `<canvas>` via
/// `ImageData`.  Width and height can be read with `camera_width()` /
/// `camera_height()`.
#[wasm_bindgen]
pub async fn capture_frame() -> Result<js_sys::Uint8Array, JsValue> {
    // Take the Camera out of the RefCell so we can hold a reference to it
    // across the (synchronous-in-practice) async call.
    let cam = CAMERA
        .with(|c| c.borrow_mut().take())
        .ok_or_else(|| JsValue::from_str("camera not started — call start_camera() first"))?;

    let result = cam.capture_frame().await;

    // Always put the camera back, even on error.
    CAMERA.with(|c| *c.borrow_mut() = Some(cam));

    let frame = result.map_err(|e| JsValue::from_str(&e.to_string()))?;

    // Update stored dimensions from the actual frame (may differ from config).
    WIDTH.with(|w| *w.borrow_mut() = frame.width);
    HEIGHT.with(|h| *h.borrow_mut() = frame.height);

    // rcam delivers BGRA; the browser's ImageData expects RGBA — swap B ↔ R.
    let mut rgba = frame.data;
    for pixel in rgba.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }

    Ok(js_sys::Uint8Array::from(rgba.as_slice()))
}

/// Begin a `MediaRecorder` session on the live stream.
///
/// Call `stop_recording()` to end the session and retrieve the WebM bytes.
#[wasm_bindgen]
pub async fn start_recording() -> Result<(), JsValue> {
    let mut cam = CAMERA
        .with(|c| c.borrow_mut().take())
        .ok_or_else(|| JsValue::from_str("camera not started — call start_camera() first"))?;

    let result = cam.start_recording(RecordingOutput::Buffer).await;

    CAMERA.with(|c| *c.borrow_mut() = Some(cam));

    result.map_err(|e| JsValue::from_str(&e.to_string()))
}

/// Stop the active recording and return the encoded video as a WebM `Uint8Array`.
///
/// Waits for the browser to flush the final chunk before returning.
#[wasm_bindgen]
pub async fn stop_recording() -> Result<js_sys::Uint8Array, JsValue> {
    let mut cam = CAMERA
        .with(|c| c.borrow_mut().take())
        .ok_or_else(|| JsValue::from_str("camera not started — call start_camera() first"))?;

    let result = cam.stop_recording().await;

    CAMERA.with(|c| *c.borrow_mut() = Some(cam));

    let video_data = result.map_err(|e| JsValue::from_str(&e.to_string()))?;

    match video_data.kind {
        VideoOutput::Buffer(bytes) => Ok(js_sys::Uint8Array::from(bytes.as_slice())),
        VideoOutput::File(_) => Err(JsValue::from_str("unexpected File output on WASM")),
    }
}

/// Stop the stream and release the camera hardware.
#[wasm_bindgen]
pub async fn stop_camera() -> Result<(), JsValue> {
    let cam = CAMERA.with(|c| c.borrow_mut().take());
    if let Some(cam) = cam {
        cam.close()
            .await
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
    }
    Ok(())
}

/// Width (pixels) of the most recently captured frame.
#[wasm_bindgen]
pub fn camera_width() -> u32 {
    WIDTH.with(|w| *w.borrow())
}

/// Height (pixels) of the most recently captured frame.
#[wasm_bindgen]
pub fn camera_height() -> u32 {
    HEIGHT.with(|h| *h.borrow())
}
