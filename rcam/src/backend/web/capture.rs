//! Browser `getUserMedia` / `enumerateDevices` — Phase 3 implementation.

use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;

use crate::{CameraConfig, CameraError, CameraInfo, CameraPosition};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Returns `navigator.mediaDevices`, or an error if the browser doesn't
/// support it (e.g. non-secure context, headless environment).
pub fn media_devices() -> Result<web_sys::MediaDevices, CameraError> {
    web_sys::window()
        .and_then(|w| w.navigator().media_devices().ok())
        .ok_or_else(|| {
            CameraError::Backend(
                "navigator.mediaDevices unavailable (requires HTTPS or localhost)".into(),
            )
        })
}

// ---------------------------------------------------------------------------
// Enumeration
// ---------------------------------------------------------------------------

/// Enumerate video-input devices via `navigator.mediaDevices.enumerateDevices`.
///
/// Browser security note: device **labels** are empty strings until
/// `getUserMedia` permission has been granted. Call `enumerate()` after
/// `open()` for human-readable names.
pub async fn enumerate_devices() -> Result<Vec<CameraInfo>, CameraError> {
    let devices = media_devices()?;

    let promise = devices
        .enumerate_devices()
        .map_err(|e| CameraError::Backend(js_err(e)))?;

    let result = JsFuture::from(promise)
        .await
        .map_err(|e| CameraError::Backend(js_err(e)))?;

    let arr = js_sys::Array::from(&result);
    let mut infos = Vec::new();

    for (i, item) in arr.iter().enumerate() {
        // Filter to video-input devices only before consuming `item`.
        // JS reflection for `kind` avoids depending on the `MediaDeviceKind` feature.
        let kind = js_sys::Reflect::get(&item, &"kind".into())
            .ok()
            .and_then(|v| v.as_string())
            .unwrap_or_default();
        if kind != "videoinput" {
            continue;
        }
        // Each entry is a MediaDeviceInfo (or InputDeviceInfo subclass).
        let info = match item.dyn_into::<web_sys::MediaDeviceInfo>() {
            Ok(d) => d,
            Err(_) => continue,
        };
        let label = info.label();
        let id = info.device_id();
        infos.push(CameraInfo {
            id,
            name: if label.is_empty() {
                format!("Camera {i}")
            } else {
                label
            },
            position: CameraPosition::Unknown,
            is_default: i == 0,
        });
    }

    Ok(infos)
}

// ---------------------------------------------------------------------------
// Stream acquisition
// ---------------------------------------------------------------------------

/// Call `getUserMedia` with constraints derived from `config`.
///
/// Returns the live `MediaStream` from the browser.
pub async fn get_stream(config: &CameraConfig) -> Result<web_sys::MediaStream, CameraError> {
    let devices = media_devices()?;

    // Build video constraint object using JS reflection.
    let video_constraint = build_video_constraint(config);

    let constraints = web_sys::MediaStreamConstraints::new();
    constraints.set_video(&video_constraint);
    // Audio is out of scope for this crate.
    constraints.set_audio(&JsValue::FALSE);

    let promise = devices
        .get_user_media_with_constraints(&constraints)
        .map_err(|e| CameraError::Backend(js_err(e)))?;

    let stream_val = JsFuture::from(promise)
        .await
        .map_err(|e| match e.as_string() {
            Some(s) if s.contains("NotAllowed") => CameraError::PermissionDenied,
            Some(s) if s.contains("NotFound") => CameraError::NoCameraFound,
            Some(s) => CameraError::Backend(s),
            None => CameraError::Backend(js_err(e)),
        })?;

    stream_val
        .dyn_into::<web_sys::MediaStream>()
        .map_err(|_| CameraError::Backend("getUserMedia did not return a MediaStream".into()))
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Build the `video` constraint value for `getUserMedia`.
fn build_video_constraint(config: &CameraConfig) -> JsValue {
    use js_sys::{Object, Reflect};

    let obj = Object::new();

    if let Some(id) = &config.device_id {
        // { deviceId: { exact: "<id>" } }
        let id_constraint = Object::new();
        Reflect::set(&id_constraint, &"exact".into(), &JsValue::from_str(id)).ok();
        Reflect::set(&obj, &"deviceId".into(), &id_constraint).ok();
    }

    // Resolution hints (the browser treats these as ideal, not hard constraints).
    Reflect::set(
        &obj,
        &"width".into(),
        &JsValue::from(config.resolution.width),
    )
    .ok();
    Reflect::set(
        &obj,
        &"height".into(),
        &JsValue::from(config.resolution.height),
    )
    .ok();
    Reflect::set(&obj, &"frameRate".into(), &JsValue::from(config.frame_rate)).ok();

    obj.into()
}

/// Convert a JavaScript error value to a Rust `String`.
pub fn js_err(e: JsValue) -> String {
    e.as_string()
        .or_else(|| {
            js_sys::Reflect::get(&e, &"message".into())
                .ok()
                .and_then(|v| v.as_string())
        })
        .unwrap_or_else(|| "unknown JS error".into())
}
