//! Camera2 NDK device management — Phase 5 implementation.
//!
//! Provides safe RAII wrappers around the Camera2 NDK C API:
//!
//! * `enumerate_devices()` — lists all cameras via `ACameraManager`.
//! * `Camera2Session` — owns the full capture pipeline for one open camera,
//!   delivering `Frame`s through a Tokio unbounded channel.
//!
//! Frame data is YUV_420_888.  We linearise the three planes (Y, U, V with
//! separate strides and pixel-strides) into a packed `Vec<u8>` using the
//! YUV 4:2:0 planar layout that most decoders expect, then tag the `Frame`
//! with `FrameFormat::YUV420`.
//!
//! # Threading
//!
//! The `AImageReader_ImageListener` callback fires on an arbitrary camera
//! background thread.  The `FrameContext` box is kept alive by `Camera2Session`
//! and accessed exclusively through a raw pointer held by the NDK, so it is
//! `Send` (all interior state is either atomic or a `Sender`).

use std::ffi::{CStr, CString};
use std::os::raw::c_void;
use std::sync::{Condvar, Mutex};
use std::time::Duration;

use tokio::sync::mpsc::UnboundedSender;

use rcam_sys_android as ffi;

use crate::{CameraError, CameraInfo, CameraPosition, Frame, FrameFormat, Resolution};

// ---------------------------------------------------------------------------
// ndk_ok! — map camera_status_t / media_status_t to CameraError
// ---------------------------------------------------------------------------

macro_rules! ndk_ok {
    ($expr:expr, $msg:literal) => {{
        let status = $expr;
        if status != 0 {
            return Err(CameraError::Backend(format!(
                "{}: NDK status {}",
                $msg, status
            )));
        }
    }};
}

// ---------------------------------------------------------------------------
// Public interface
// ---------------------------------------------------------------------------

/// Enumerate all available camera devices.
pub fn enumerate_devices() -> Result<Vec<CameraInfo>, CameraError> {
    let manager = Manager::new()?;

    let mut id_list_ptr: *mut ffi::ACameraIdList = std::ptr::null_mut();
    unsafe {
        ndk_ok!(
            ffi::ACameraManager_getCameraIdList(manager.0, &mut id_list_ptr),
            "ACameraManager_getCameraIdList"
        );
    }

    // SAFETY: `id_list_ptr` is non-null after a successful call.
    let id_list = unsafe { &*id_list_ptr };
    let num = id_list.numCameras as usize;

    let mut result = Vec::with_capacity(num);

    for i in 0..num {
        // SAFETY: `cameraIds` is an array of `numCameras` valid C strings.
        let raw_id = unsafe { *id_list.cameraIds.add(i) };
        let id = unsafe { CStr::from_ptr(raw_id) }
            .to_string_lossy()
            .into_owned();

        let position = query_lens_facing(manager.0, raw_id);
        let is_default = i == 0;

        result.push(CameraInfo {
            id: id.clone(),
            name: format!("Android Camera {}", &id),
            position,
            is_default,
        });
    }

    unsafe { ffi::ACameraManager_deleteCameraIdList(id_list_ptr) };
    Ok(result)
}

/// Query `ACAMERA_LENS_FACING` for a single camera ID.
fn query_lens_facing(
    manager: *mut ffi::ACameraManager,
    raw_id: *const std::os::raw::c_char,
) -> CameraPosition {
    let mut meta_ptr: *mut ffi::ACameraMetadata = std::ptr::null_mut();
    let ok =
        unsafe { ffi::ACameraManager_getCameraCharacteristics(manager, raw_id, &mut meta_ptr) };
    if ok != 0 || meta_ptr.is_null() {
        return CameraPosition::Unknown;
    }

    let mut entry: ffi::ACameraMetadata_const_entry = unsafe { std::mem::zeroed() };
    let ok = unsafe {
        ffi::ACameraMetadata_getConstEntry(meta_ptr, ffi::ACAMERA_LENS_FACING, &mut entry)
    };

    let position = if ok == 0 && entry.count > 0 {
        // SAFETY: `entry.data.u8_` is valid for `entry.count` bytes after a
        // successful `getConstEntry` with a BYTE-type tag.
        let facing = unsafe { *entry.data.u8_ };
        match facing {
            ffi::ACAMERA_LENS_FACING_FRONT => CameraPosition::Front,
            ffi::ACAMERA_LENS_FACING_BACK => CameraPosition::Back,
            ffi::ACAMERA_LENS_FACING_EXTERNAL => CameraPosition::External,
            _ => CameraPosition::Unknown,
        }
    } else {
        CameraPosition::Unknown
    };

    unsafe { ffi::ACameraMetadata_free(meta_ptr) };
    position
}

// ---------------------------------------------------------------------------
// RAII wrappers
// ---------------------------------------------------------------------------

/// RAII guard for `*mut ACameraManager`.
struct Manager(*mut ffi::ACameraManager);

impl Manager {
    fn new() -> Result<Self, CameraError> {
        let ptr = unsafe { ffi::ACameraManager_create() };
        if ptr.is_null() {
            Err(CameraError::Backend(
                "ACameraManager_create returned null".into(),
            ))
        } else {
            Ok(Self(ptr))
        }
    }
}

impl Drop for Manager {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { ffi::ACameraManager_delete(self.0) };
        }
    }
}

// ---------------------------------------------------------------------------
// Frame context — shared between Camera2Session and the NDK callback thread
// ---------------------------------------------------------------------------

/// Heap-allocated context passed to `AImageReader_ImageListener`.
/// Kept alive by `Camera2Session`; freed on drop.
pub(super) struct FrameContext {
    pub tx: UnboundedSender<Frame>,
}

/// Image-available callback: acquires the latest image, packs it as YUV420,
/// and pushes it into the frame channel.
unsafe extern "C" fn on_image_available(context: *mut c_void, reader: *mut ffi::AImageReader) {
    let ctx = &*(context as *const FrameContext);

    let mut image_ptr: *mut ffi::AImage = std::ptr::null_mut();
    if ffi::AImageReader_acquireLatestImage(reader, &mut image_ptr) != ffi::AMEDIA_OK
        || image_ptr.is_null()
    {
        return;
    }

    if let Some(frame) = extract_frame(image_ptr) {
        let _ = ctx.tx.send(frame);
    }

    ffi::AImage_delete(image_ptr);
}

/// Pack a YUV_420_888 `AImage` into an I420-layout `Frame`.
///
/// I420: Y-plane (WxH), then U-plane (W/2 × H/2), then V-plane (W/2 × H/2).
unsafe fn extract_frame(image: *mut ffi::AImage) -> Option<Frame> {
    let mut width: i32 = 0;
    let mut height: i32 = 0;
    let mut ts_ns: i64 = 0;

    if ffi::AImage_getWidth(image, &mut width) != ffi::AMEDIA_OK {
        return None;
    }
    if ffi::AImage_getHeight(image, &mut height) != ffi::AMEDIA_OK {
        return None;
    }
    let _ = ffi::AImage_getTimestamp(image, &mut ts_ns);

    let w = width as usize;
    let h = height as usize;
    let mut out = Vec::with_capacity(w * h + (w / 2) * (h / 2) * 2);

    // Planes: 0 = Y, 1 = Cb/U, 2 = Cr/V
    for plane_idx in 0i32..3 {
        let mut data_ptr: *const u8 = std::ptr::null();
        let mut data_len: i32 = 0;
        let mut row_stride: i32 = 1;
        let mut px_stride: i32 = 1;

        if ffi::AImage_getPlaneData(image, plane_idx, &mut data_ptr, &mut data_len)
            != ffi::AMEDIA_OK
        {
            return None;
        }
        let _ = ffi::AImage_getPlaneRowStride(image, plane_idx, &mut row_stride);
        let _ = ffi::AImage_getPlanePixelStride(image, plane_idx, &mut px_stride);

        let (plane_w, plane_h) = if plane_idx == 0 {
            (w, h)
        } else {
            (w / 2, h / 2)
        };

        // Copy plane into I420 layout, stripping any row/pixel padding.
        for row in 0..plane_h {
            let row_base = unsafe { data_ptr.add(row * row_stride as usize) };
            if px_stride == 1 {
                // Packed row — memcpy the whole row.
                let slice = std::slice::from_raw_parts(row_base, plane_w);
                out.extend_from_slice(slice);
            } else {
                // Strided pixels — copy one sample at a time.
                for col in 0..plane_w {
                    out.push(*row_base.add(col * px_stride as usize));
                }
            }
        }
    }

    Some(Frame {
        data: out,
        width: width as u32,
        height: height as u32,
        format: FrameFormat::YUV420,
        timestamp_us: (ts_ns / 1_000) as u64,
    })
}

// ---------------------------------------------------------------------------
// Session context — used to signal when the capture session becomes active
// ---------------------------------------------------------------------------

struct SessionContext {
    state: Mutex<SessionState>,
    cvar: Condvar,
}

#[derive(PartialEq)]
enum SessionState {
    Pending,
    Active,
    Error,
}

unsafe extern "C" fn on_session_active(ctx: *mut c_void, _: *mut ffi::ACameraCaptureSession) {
    let sc = &*(ctx as *const SessionContext);
    *sc.state.lock().unwrap() = SessionState::Active;
    sc.cvar.notify_one();
}

unsafe extern "C" fn on_session_closed(ctx: *mut c_void, _: *mut ffi::ACameraCaptureSession) {
    // Not used for wakeup, but harmless to leave in for debugging.
    let _ = ctx;
}

unsafe extern "C" fn on_session_ready(_ctx: *mut c_void, _: *mut ffi::ACameraCaptureSession) {}

unsafe extern "C" fn on_device_disconnected(ctx: *mut c_void, _: *mut ffi::ACameraDevice) {
    let sc = &*(ctx as *const SessionContext);
    *sc.state.lock().unwrap() = SessionState::Error;
    sc.cvar.notify_one();
}

unsafe extern "C" fn on_device_error(
    ctx: *mut c_void,
    _: *mut ffi::ACameraDevice,
    _error: std::os::raw::c_int,
) {
    let sc = &*(ctx as *const SessionContext);
    *sc.state.lock().unwrap() = SessionState::Error;
    sc.cvar.notify_one();
}

// ---------------------------------------------------------------------------
// Camera2Session
// ---------------------------------------------------------------------------

/// Owns the full Camera2 NDK capture pipeline for one open camera.
///
/// Frames are delivered into `frame_tx` from the NDK image-available callback.
/// All NDK objects are freed in reverse-creation order by `Drop`.
pub(super) struct Camera2Session {
    // NDK objects — freed in drop() in reverse order.
    manager: *mut ffi::ACameraManager,
    device: *mut ffi::ACameraDevice,
    reader: *mut ffi::AImageReader,
    reader_window: *mut ffi::ANativeWindow,
    session: *mut ffi::ACameraCaptureSession,
    request: *mut ffi::ACaptureRequest,
    output_target: *mut ffi::ACameraOutputTarget,
    session_output: *mut ffi::ACaptureSessionOutput,
    container: *mut ffi::ACaptureSessionOutputContainer,
    // Image listener struct — kept alive so the NDK can fire the callback.
    _listener: Box<ffi::AImageReader_ImageListener>,
    // Frame context box — keep alive until Drop.
    _frame_ctx: Box<FrameContext>,
    // Session context box — keep alive until Drop.
    _session_ctx: Box<SessionContext>,
    // Resolution for metadata on frames (from config).
    pub resolution: Resolution,
}

// SAFETY: All NDK objects are internally thread-safe (Camera2 API contract).
// We serialise Rust-side access via the Mutex in `AndroidCamera`.
unsafe impl Send for Camera2Session {}
unsafe impl Sync for Camera2Session {}

impl Camera2Session {
    /// Open a camera and start a repeating preview capture.
    ///
    /// Blocks until the capture session becomes active (with a 5 s timeout),
    /// then returns.  Frames will flow into `frame_tx` asynchronously.
    pub fn open(
        device_id: Option<&str>,
        position: CameraPosition,
        resolution: Resolution,
        frame_rate: u32,
        frame_tx: UnboundedSender<Frame>,
    ) -> Result<Self, CameraError> {
        // --- Manager ---
        let manager = unsafe { ffi::ACameraManager_create() };
        if manager.is_null() {
            return Err(CameraError::Backend("ACameraManager_create failed".into()));
        }

        // --- Resolve camera ID ---
        let camera_id_str = resolve_camera_id(manager, device_id, position)?;
        let camera_id_c = CString::new(camera_id_str.as_str())
            .map_err(|_| CameraError::Backend("invalid camera ID string".into()))?;

        // --- Session context (for open / session callbacks) ---
        let session_ctx = Box::new(SessionContext {
            state: Mutex::new(SessionState::Pending),
            cvar: Condvar::new(),
        });
        let sc_ptr = &*session_ctx as *const SessionContext as *mut c_void;

        // --- Open camera device ---
        let device_callbacks = ffi::ACameraDevice_StateCallbacks {
            context: sc_ptr,
            onDisconnected: Some(on_device_disconnected),
            onError: Some(on_device_error),
        };
        let mut device: *mut ffi::ACameraDevice = std::ptr::null_mut();
        unsafe {
            ndk_ok!(
                ffi::ACameraManager_openCamera(
                    manager,
                    camera_id_c.as_ptr(),
                    &device_callbacks,
                    &mut device,
                ),
                "ACameraManager_openCamera"
            );
        }

        // --- Image reader (YUV_420_888, up to 4 buffered images) ---
        let mut reader: *mut ffi::AImageReader = std::ptr::null_mut();
        unsafe {
            ndk_ok!(
                ffi::AImageReader_new(
                    resolution.width as i32,
                    resolution.height as i32,
                    ffi::AIMAGE_FORMAT_YUV_420_888,
                    4,
                    &mut reader,
                ),
                "AImageReader_new"
            );
        }

        // --- Frame context + image listener ---
        let frame_ctx = Box::new(FrameContext { tx: frame_tx });
        let fc_ptr = &*frame_ctx as *const FrameContext as *mut c_void;

        let mut listener = Box::new(ffi::AImageReader_ImageListener {
            context: fc_ptr,
            onImageAvailable: Some(on_image_available),
        });
        unsafe {
            ndk_ok!(
                ffi::AImageReader_setImageListener(reader, &mut *listener),
                "AImageReader_setImageListener"
            );
        }

        // --- Get the reader's ANativeWindow ---
        let mut reader_window: *mut ffi::ANativeWindow = std::ptr::null_mut();
        unsafe {
            ndk_ok!(
                ffi::AImageReader_getWindow(reader, &mut reader_window),
                "AImageReader_getWindow"
            );
        }

        // --- Build output container ---
        let mut container: *mut ffi::ACaptureSessionOutputContainer = std::ptr::null_mut();
        unsafe {
            ndk_ok!(
                ffi::ACaptureSessionOutputContainer_create(&mut container),
                "ACaptureSessionOutputContainer_create"
            );
        }

        let mut session_output: *mut ffi::ACaptureSessionOutput = std::ptr::null_mut();
        unsafe {
            ndk_ok!(
                ffi::ACaptureSessionOutput_create(reader_window, &mut session_output),
                "ACaptureSessionOutput_create"
            );
            ndk_ok!(
                ffi::ACaptureSessionOutputContainer_add(container, session_output),
                "ACaptureSessionOutputContainer_add"
            );
        }

        // --- Create capture session ---
        let session_callbacks = ffi::ACameraCaptureSession_stateCallbacks {
            context: sc_ptr,
            onClosed: Some(on_session_closed),
            onReady: Some(on_session_ready),
            onActive: Some(on_session_active),
        };
        let mut session: *mut ffi::ACameraCaptureSession = std::ptr::null_mut();
        unsafe {
            ndk_ok!(
                ffi::ACameraDevice_createCaptureSession(
                    device,
                    container,
                    &session_callbacks,
                    &mut session,
                ),
                "ACameraDevice_createCaptureSession"
            );
        }

        // Wait for the session to become active (or error) before proceeding.
        {
            let lock = session_ctx.state.lock().unwrap();
            let (guard, timed_out) = session_ctx
                .cvar
                .wait_timeout_while(lock, Duration::from_secs(5), |s| {
                    *s == SessionState::Pending
                })
                .unwrap();
            if timed_out.timed_out() || *guard == SessionState::Error {
                return Err(CameraError::Backend(
                    "capture session failed to become active".into(),
                ));
            }
        }

        // --- Build capture request (PREVIEW template) ---
        let mut request: *mut ffi::ACaptureRequest = std::ptr::null_mut();
        unsafe {
            ndk_ok!(
                ffi::ACameraDevice_createCaptureRequest(
                    device,
                    ffi::TEMPLATE_PREVIEW,
                    &mut request
                ),
                "ACameraDevice_createCaptureRequest"
            );
        }

        // --- Add image reader window as request target ---
        let mut output_target: *mut ffi::ACameraOutputTarget = std::ptr::null_mut();
        unsafe {
            ndk_ok!(
                ffi::ACameraOutputTarget_create(reader_window, &mut output_target),
                "ACameraOutputTarget_create"
            );
            ndk_ok!(
                ffi::ACaptureRequest_addTarget(request, output_target),
                "ACaptureRequest_addTarget"
            );
        }

        // --- Start repeating preview ---
        let mut seq_id: std::os::raw::c_int = 0;
        let mut req_ptr = request;
        unsafe {
            ndk_ok!(
                ffi::ACameraCaptureSession_setRepeatingRequest(
                    session,
                    std::ptr::null_mut(),
                    1,
                    &mut req_ptr,
                    &mut seq_id,
                ),
                "ACameraCaptureSession_setRepeatingRequest"
            );
        }

        let _ = frame_rate; // frame-rate control via request metadata — Phase 6

        Ok(Self {
            manager,
            device,
            reader,
            reader_window,
            session,
            request,
            output_target,
            session_output,
            container,
            _listener: listener,
            _frame_ctx: frame_ctx,
            _session_ctx: session_ctx,
            resolution,
        })
    }

    /// Add an extra `ANativeWindow` output to this session (used for recording).
    ///
    /// Stops the repeating request, closes the current session, rebuilds the
    /// output container with the new window added, then restarts everything.
    pub fn add_output(
        &mut self,
        extra_window: *mut ffi::ANativeWindow,
    ) -> Result<
        (
            *mut ffi::ACaptureSessionOutput,
            *mut ffi::ACameraOutputTarget,
        ),
        CameraError,
    > {
        unsafe {
            // Stop repeating and close the old session.
            let _ = ffi::ACameraCaptureSession_stopRepeating(self.session);
            ffi::ACameraCaptureSession_close(self.session);

            // Build a new container with both outputs.
            let mut new_container: *mut ffi::ACaptureSessionOutputContainer = std::ptr::null_mut();
            ndk_ok!(
                ffi::ACaptureSessionOutputContainer_create(&mut new_container),
                "ACaptureSessionOutputContainer_create (recording)"
            );
            ndk_ok!(
                ffi::ACaptureSessionOutputContainer_add(new_container, self.session_output),
                "add reader output"
            );

            let mut extra_output: *mut ffi::ACaptureSessionOutput = std::ptr::null_mut();
            ndk_ok!(
                ffi::ACaptureSessionOutput_create(extra_window, &mut extra_output),
                "ACaptureSessionOutput_create (recorder)"
            );
            ndk_ok!(
                ffi::ACaptureSessionOutputContainer_add(new_container, extra_output),
                "add recorder output"
            );

            // Recreate session.
            let sc_ptr = &*self._session_ctx as *const SessionContext as *mut c_void;
            // Reset session state.
            *self._session_ctx.state.lock().unwrap() = SessionState::Pending;
            let session_callbacks = ffi::ACameraCaptureSession_stateCallbacks {
                context: sc_ptr,
                onClosed: Some(on_session_closed),
                onReady: Some(on_session_ready),
                onActive: Some(on_session_active),
            };
            let mut new_session: *mut ffi::ACameraCaptureSession = std::ptr::null_mut();
            ndk_ok!(
                ffi::ACameraDevice_createCaptureSession(
                    self.device,
                    new_container,
                    &session_callbacks,
                    &mut new_session,
                ),
                "ACameraDevice_createCaptureSession (recording)"
            );

            // Wait for session.
            {
                let lock = self._session_ctx.state.lock().unwrap();
                let (guard, timed_out) = self
                    ._session_ctx
                    .cvar
                    .wait_timeout_while(lock, Duration::from_secs(5), |s| {
                        *s == SessionState::Pending
                    })
                    .unwrap();
                if timed_out.timed_out() || *guard == SessionState::Error {
                    return Err(CameraError::Backend("recording session failed".into()));
                }
            }

            // Free old container.
            ffi::ACaptureSessionOutputContainer_free(self.container);
            self.container = new_container;
            self.session = new_session;

            // Add new window as a capture target.
            let mut extra_target: *mut ffi::ACameraOutputTarget = std::ptr::null_mut();
            ndk_ok!(
                ffi::ACameraOutputTarget_create(extra_window, &mut extra_target),
                "ACameraOutputTarget_create (recorder)"
            );
            ndk_ok!(
                ffi::ACaptureRequest_addTarget(self.request, extra_target),
                "ACaptureRequest_addTarget (recorder)"
            );

            // Restart repeating request.
            let mut req_ptr = self.request;
            let mut seq_id: std::os::raw::c_int = 0;
            ndk_ok!(
                ffi::ACameraCaptureSession_setRepeatingRequest(
                    self.session,
                    std::ptr::null_mut(),
                    1,
                    &mut req_ptr,
                    &mut seq_id,
                ),
                "setRepeatingRequest (recording)"
            );

            Ok((extra_output, extra_target))
        }
    }

    /// Remove a previously-added extra output (called after recording stops).
    pub fn remove_output(
        &mut self,
        extra_output: *mut ffi::ACaptureSessionOutput,
        extra_target: *mut ffi::ACameraOutputTarget,
        extra_window: *mut ffi::ANativeWindow,
    ) -> Result<(), CameraError> {
        unsafe {
            let _ = ffi::ACameraCaptureSession_stopRepeating(self.session);
            ffi::ACaptureRequest_removeTarget(self.request, extra_target);
            ffi::ACameraOutputTarget_free(extra_target);

            ffi::ACameraCaptureSession_close(self.session);

            // Rebuild container with only the image reader output.
            let mut new_container: *mut ffi::ACaptureSessionOutputContainer = std::ptr::null_mut();
            ndk_ok!(
                ffi::ACaptureSessionOutputContainer_create(&mut new_container),
                "ACaptureSessionOutputContainer_create (post-recording)"
            );
            ndk_ok!(
                ffi::ACaptureSessionOutputContainer_add(new_container, self.session_output),
                "add reader output (post-recording)"
            );

            let sc_ptr = &*self._session_ctx as *const SessionContext as *mut c_void;
            *self._session_ctx.state.lock().unwrap() = SessionState::Pending;
            let session_callbacks = ffi::ACameraCaptureSession_stateCallbacks {
                context: sc_ptr,
                onClosed: Some(on_session_closed),
                onReady: Some(on_session_ready),
                onActive: Some(on_session_active),
            };
            let mut new_session: *mut ffi::ACameraCaptureSession = std::ptr::null_mut();
            ndk_ok!(
                ffi::ACameraDevice_createCaptureSession(
                    self.device,
                    new_container,
                    &session_callbacks,
                    &mut new_session,
                ),
                "ACameraDevice_createCaptureSession (post-recording)"
            );

            {
                let lock = self._session_ctx.state.lock().unwrap();
                let (guard, timed_out) = self
                    ._session_ctx
                    .cvar
                    .wait_timeout_while(lock, Duration::from_secs(5), |s| {
                        *s == SessionState::Pending
                    })
                    .unwrap();
                if timed_out.timed_out() || *guard == SessionState::Error {
                    return Err(CameraError::Backend("post-recording session failed".into()));
                }
            }

            ffi::ACaptureSessionOutputContainer_free(self.container);
            self.container = new_container;
            self.session = new_session;

            // Free the recording session output and its window.
            ffi::ACaptureSessionOutput_free(extra_output);
            let _ = extra_window; // owned by the AMediaRecorder; it will free it

            // Restart preview.
            let mut req_ptr = self.request;
            let mut seq_id: std::os::raw::c_int = 0;
            ndk_ok!(
                ffi::ACameraCaptureSession_setRepeatingRequest(
                    self.session,
                    std::ptr::null_mut(),
                    1,
                    &mut req_ptr,
                    &mut seq_id,
                ),
                "setRepeatingRequest (post-recording)"
            );

            Ok(())
        }
    }
}

impl Drop for Camera2Session {
    fn drop(&mut self) {
        // Tear down in reverse creation order.
        unsafe {
            if !self.session.is_null() {
                let _ = ffi::ACameraCaptureSession_stopRepeating(self.session);
                ffi::ACameraCaptureSession_close(self.session);
            }
            if !self.request.is_null() {
                let _ = ffi::ACaptureRequest_removeTarget(self.request, self.output_target);
                ffi::ACaptureRequest_free(self.request);
            }
            if !self.output_target.is_null() {
                ffi::ACameraOutputTarget_free(self.output_target);
            }
            if !self.session_output.is_null() {
                ffi::ACaptureSessionOutput_free(self.session_output);
            }
            if !self.container.is_null() {
                ffi::ACaptureSessionOutputContainer_free(self.container);
            }
            if !self.reader.is_null() {
                ffi::AImageReader_delete(self.reader);
            }
            if !self.device.is_null() {
                let _ = ffi::ACameraDevice_close(self.device);
            }
            if !self.manager.is_null() {
                ffi::ACameraManager_delete(self.manager);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helper — resolve camera ID from config
// ---------------------------------------------------------------------------

fn resolve_camera_id(
    manager: *mut ffi::ACameraManager,
    device_id: Option<&str>,
    position: CameraPosition,
) -> Result<String, CameraError> {
    if let Some(id) = device_id {
        return Ok(id.to_owned());
    }

    // Enumerate to find a device matching the requested position.
    let mut id_list_ptr: *mut ffi::ACameraIdList = std::ptr::null_mut();
    let status = unsafe { ffi::ACameraManager_getCameraIdList(manager, &mut id_list_ptr) };
    if status != ffi::ACAMERA_OK || id_list_ptr.is_null() {
        return Err(CameraError::NoCameraFound);
    }

    let id_list = unsafe { &*id_list_ptr };
    let num = id_list.numCameras as usize;
    if num == 0 {
        unsafe { ffi::ACameraManager_deleteCameraIdList(id_list_ptr) };
        return Err(CameraError::NoCameraFound);
    }

    // First pass: find matching position.
    let mut chosen: Option<String> = None;
    for i in 0..num {
        let raw_id = unsafe { *id_list.cameraIds.add(i) };
        let id = unsafe { CStr::from_ptr(raw_id) }
            .to_string_lossy()
            .into_owned();
        let cam_pos = query_lens_facing(manager, raw_id);
        if position == cam_pos || position == CameraPosition::Unknown {
            chosen = Some(id);
            break;
        }
    }

    // Fallback: just use the first camera.
    if chosen.is_none() {
        let raw_id = unsafe { *id_list.cameraIds };
        chosen = Some(
            unsafe { CStr::from_ptr(raw_id) }
                .to_string_lossy()
                .into_owned(),
        );
    }

    unsafe { ffi::ACameraManager_deleteCameraIdList(id_list_ptr) };
    chosen.ok_or(CameraError::NoCameraFound)
}
