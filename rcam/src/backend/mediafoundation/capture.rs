//! Windows Media Foundation device enumeration — Phase 2 implementation.
//!
//! Uses `Windows.Devices.Enumeration.DeviceInformation` to list video-capture
//! capable hardware. All WinRT async operations are awaited directly; they use
//! WinRT's internal thread pool and are compatible with any Rust async runtime.

use windows::Devices::Enumeration::{DeviceClass, DeviceInformation};

use crate::{CameraError, CameraInfo, CameraPosition};

/// Enumerate video-capture devices via `DeviceInformation::FindAllAsync`.
///
/// Uses the `VideoCapture` device class selector, which covers built-in
/// webcams and externally connected USB/UVC cameras.
pub async fn enumerate_devices() -> Result<Vec<CameraInfo>, CameraError> {
    let devices =
        DeviceInformation::FindAllAsyncDeviceClass(DeviceClass::VideoCapture)
            .map_err(|e| CameraError::Backend(e.to_string()))?
            .await
            .map_err(|e| CameraError::Backend(e.to_string()))?;

    let count = devices
        .Size()
        .map_err(|e| CameraError::Backend(e.to_string()))?;

    let mut infos = Vec::with_capacity(count as usize);
    for i in 0..count {
        let info = devices
            .GetAt(i)
            .map_err(|e| CameraError::Backend(e.to_string()))?;
        let name = info
            .Name()
            .map_err(|e| CameraError::Backend(e.to_string()))?
            .to_string();
        let id = info
            .Id()
            .map_err(|e| CameraError::Backend(e.to_string()))?
            .to_string();

        infos.push(CameraInfo {
            id,
            name,
            position: CameraPosition::Unknown,
            // WinRT DeviceInformation has no canonical "is default" flag for
            // cameras; treat the first enumerated device as the default.
            is_default: i == 0,
        });
    }
    Ok(infos)
}
