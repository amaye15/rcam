//! Camera2 NDK device management (Phase 1 stub).
//!
//! Phase 5 implementation will:
//!   1. Call `ACameraManager_create()` to get a manager handle.
//!   2. Call `ACameraManager_getCameraIdList()` to enumerate devices.
//!   3. Query `ACameraMetadata` for capabilities per device.
//!   4. Open a device with `ACameraManager_openCamera()`.
//!   5. Create `AImageReader` and `ACameraCaptureSession` for streaming.

use crate::{CameraError, CameraInfo};

pub fn enumerate_devices() -> Result<Vec<CameraInfo>, CameraError> {
    // Phase 5: use rcam-sys-android bindings.
    Ok(vec![])
}
