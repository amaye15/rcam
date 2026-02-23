//! Windows Media Foundation device enumeration — Phase 2 implementation.
//!
//! Uses `Windows.Devices.Enumeration.DeviceInformation` to list video-capture
//! capable hardware.
//!
//! # Threading note
//!
//! `IAsyncOperation<T>` from `windows-rs` does not implement `std::future::Future`
//! in all configurations. We use a synchronous spin-wait helper (`wrt_get`) to
//! resolve async operations without requiring a WinRT-aware async runtime.
//! All callers must run this code on a blocking thread (e.g. via
//! `tokio::task::spawn_blocking`).

use windows::Devices::Enumeration::{DeviceClass, DeviceInformation};
use windows::Foundation::{AsyncStatus, IAsyncOperation};

use crate::{CameraError, CameraInfo, CameraPosition};

// ---------------------------------------------------------------------------
// Synchronous WinRT async helper
// ---------------------------------------------------------------------------

/// Block the current thread until a WinRT `IAsyncOperation<T>` resolves.
///
/// Spins using `std::hint::spin_loop()` — acceptable on a dedicated blocking
/// thread (i.e. inside `tokio::task::spawn_blocking`). Most WinRT enumeration
/// calls resolve in milliseconds.
pub(super) fn wrt_get<T>(op: IAsyncOperation<T>) -> windows::core::Result<T>
where
    T: windows::core::RuntimeType + 'static,
{
    loop {
        let status = op.Status()?;
        if status == AsyncStatus::Completed {
            return op.GetResults();
        }
        if status != AsyncStatus::Started {
            // Error or Canceled — GetResults() will return the failure HRESULT.
            return op.GetResults();
        }
        std::hint::spin_loop();
    }
}

// ---------------------------------------------------------------------------
// Enumeration
// ---------------------------------------------------------------------------

/// Enumerate video-capture devices via `DeviceInformation::FindAllAsync`.
///
/// Uses the `VideoCapture` device class selector, which covers built-in
/// webcams and externally connected USB/UVC cameras.
///
/// Must be called on a blocking thread; uses [`wrt_get`] internally.
pub fn enumerate_devices() -> Result<Vec<CameraInfo>, CameraError> {
    let op = DeviceInformation::FindAllAsyncDeviceClass(DeviceClass::VideoCapture)
        .map_err(|e| CameraError::Backend(e.to_string()))?;

    let devices = wrt_get(op).map_err(|e| CameraError::Backend(e.to_string()))?;

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
