//! Browser getUserMedia frame capture (Phase 1 stub).
//!
//! Phase 3 implementation will:
//!   1. Call `navigator.mediaDevices().enumerate_devices()` for device list.
//!      Note: full labels are only available after `getUserMedia` has been granted.
//!   2. Call `navigator.mediaDevices().get_user_media_with_constraints()` wrapped
//!      in `wasm_bindgen_futures::JsFuture` to obtain a `MediaStream`.
//!   3. For frame capture: draw the stream onto a `<canvas>` and call
//!      `getImageData()` to extract raw RGBA pixels into a `Frame`.
//!   4. `ImageCapture` API (Chromium-only) may be used for still photos.

use crate::{CameraError, CameraInfo};

pub async fn enumerate_devices() -> Result<Vec<CameraInfo>, CameraError> {
    // Phase 3: use web-sys MediaDevices APIs.
    Ok(vec![])
}
