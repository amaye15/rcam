//! AVCaptureDevice enumeration and permission — Phase 4 implementation.

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::Bool as ObjcBool;
use objc2_av_foundation::{
    AVAuthorizationStatus, AVCaptureDevice, AVCaptureDevicePosition, AVMediaTypeVideo,
};
use objc2_foundation::NSString;

use crate::{CameraError, CameraInfo, CameraPosition};

/// Enumerate connected cameras via AVFoundation.
// devicesWithMediaType is deprecated in favour of AVCaptureDeviceDiscoverySession,
// but remains the simplest cross-version enumeration path for now.
#[allow(deprecated)]
pub fn enumerate_devices() -> Result<Vec<CameraInfo>, CameraError> {
    // SAFETY: AVMediaTypeVideo is a valid static string constant.
    let media_type = unsafe { AVMediaTypeVideo }.ok_or(CameraError::NoCameraFound)?;

    // Get all video devices.
    let devices = unsafe { AVCaptureDevice::devicesWithMediaType(media_type) };

    // Find the default device ID for marking is_default.
    let default_id: Option<String> =
        unsafe { AVCaptureDevice::defaultDeviceWithMediaType(media_type) }
            .map(|d| unsafe { d.uniqueID() }.to_string());

    let mut result = Vec::new();
    for device in devices.iter() {
        let id = unsafe { device.uniqueID() }.to_string();
        let name = unsafe { device.localizedName() }.to_string();
        let position = match unsafe { device.position() } {
            AVCaptureDevicePosition::Front => CameraPosition::Front,
            AVCaptureDevicePosition::Back => CameraPosition::Back,
            _ => CameraPosition::Unknown,
        };
        let is_default = default_id.as_deref() == Some(id.as_str());
        result.push(CameraInfo {
            id,
            name,
            position,
            is_default,
        });
    }

    Ok(result)
}

/// Look up a specific device by its unique ID.
pub fn device_with_id(id: &str) -> Option<Retained<AVCaptureDevice>> {
    let ns_id = NSString::from_str(id);
    unsafe { AVCaptureDevice::deviceWithUniqueID(&ns_id) }
}

/// Return the default video capture device.
pub fn default_device() -> Option<Retained<AVCaptureDevice>> {
    let media_type = unsafe { AVMediaTypeVideo }?;
    unsafe { AVCaptureDevice::defaultDeviceWithMediaType(media_type) }
}

/// Return the first video device matching `position`, or the system default
/// when `position` is `Unknown` / `External`.
#[allow(deprecated)]
pub fn device_for_position(position: CameraPosition) -> Option<Retained<AVCaptureDevice>> {
    let avf_pos = match position {
        CameraPosition::Front => AVCaptureDevicePosition::Front,
        CameraPosition::Back => AVCaptureDevicePosition::Back,
        // No meaningful position — fall back to the system default.
        _ => return default_device(),
    };

    let media_type = unsafe { AVMediaTypeVideo }?;
    let devices = unsafe { AVCaptureDevice::devicesWithMediaType(media_type) };

    for device in devices.iter() {
        if unsafe { device.position() } == avf_pos {
            // `devices.iter()` yields `Retained<AVCaptureDevice>`; clone retains.
            return Some(device.clone());
        }
    }

    None
}

/// Check (and if needed request) camera access permission.
///
/// This function **blocks** the calling thread until the user responds to the
/// permission prompt or the OS returns a cached decision.  Always call it via
/// `tokio::task::spawn_blocking` from async contexts.
pub fn request_permission() -> Result<(), CameraError> {
    let media_type = unsafe { AVMediaTypeVideo }.ok_or(CameraError::NoCameraFound)?;

    // Return immediately if the decision is already cached.
    let status = unsafe { AVCaptureDevice::authorizationStatusForMediaType(media_type) };
    if status == AVAuthorizationStatus::Authorized {
        return Ok(());
    }
    if status == AVAuthorizationStatus::Denied || status == AVAuthorizationStatus::Restricted {
        return Err(CameraError::PermissionDenied);
    }

    // Status is `NotDetermined` — show the system prompt and wait for the user.
    let (tx, rx) = std::sync::mpsc::sync_channel::<bool>(1);

    // The completion handler uses ObjC `BOOL` (objc2::runtime::Bool).
    let block: RcBlock<dyn Fn(ObjcBool)> = RcBlock::new(move |granted: ObjcBool| {
        let _ = tx.send(granted.as_bool());
    });

    unsafe {
        AVCaptureDevice::requestAccessForMediaType_completionHandler(media_type, &block);
    }

    // Wait up to 60 s for the user to respond to the system prompt.
    // A timeout prevents an infinite hang if the callback is never delivered
    // (e.g. when the binary lacks NSCameraUsageDescription in its Info.plist).
    let granted = rx
        .recv_timeout(std::time::Duration::from_secs(60))
        .unwrap_or(false);
    if granted {
        Ok(())
    } else {
        Err(CameraError::PermissionDenied)
    }
}
