//! Raw Camera2 NDK + AMediaRecorder sys bindings for Android.
//!
//! Hand-written FFI declarations matching the NDK headers shipped with NDK r26+.
//! The `bindgen` Cargo feature (plus `ANDROID_NDK_ROOT`) can regenerate these
//! from the real headers if a header has changed.
//!
//! Minimum Android API level: 24 (Android 7.0) for Camera2 NDK;
//! API 26 for `AMediaRecorder_getInputSurface`.

#![allow(
    non_upper_case_globals,
    non_camel_case_types,
    non_snake_case,
    dead_code,
    clippy::all
)]

// Only compile on Android.
#[cfg(target_os = "android")]
pub use android_ffi::*;

#[cfg(target_os = "android")]
mod android_ffi {
    use std::os::raw::{c_char, c_int, c_void};

    // -----------------------------------------------------------------------
    // Primitive type aliases
    // -----------------------------------------------------------------------

    pub type camera_status_t = i32;
    pub type media_status_t = i32;

    // -----------------------------------------------------------------------
    // camera_status_t constants
    // -----------------------------------------------------------------------

    pub const ACAMERA_OK: camera_status_t = 0;
    pub const ACAMERA_ERROR_UNKNOWN: camera_status_t = -10000;
    pub const ACAMERA_ERROR_INVALID_PARAMETER: camera_status_t = -10001;
    pub const ACAMERA_ERROR_PERMISSION_DENIED: camera_status_t = -10002;
    pub const ACAMERA_ERROR_NOT_ENOUGH_MEMORY: camera_status_t = -10003;
    pub const ACAMERA_ERROR_METADATA_NOT_FOUND: camera_status_t = -10004;
    pub const ACAMERA_ERROR_CAMERA_DEVICE: camera_status_t = -10005;
    pub const ACAMERA_ERROR_CAMERA_SERVICE: camera_status_t = -10006;
    pub const ACAMERA_ERROR_SESSION_CLOSED: camera_status_t = -10007;
    pub const ACAMERA_ERROR_INVALID_OPERATION: camera_status_t = -10008;
    pub const ACAMERA_ERROR_STREAM_CONFIGURE_FAIL: camera_status_t = -10009;
    pub const ACAMERA_ERROR_CAMERA_IN_USE: camera_status_t = -10010;
    pub const ACAMERA_ERROR_MAX_CAMERAS_IN_USE: camera_status_t = -10011;
    pub const ACAMERA_ERROR_CAMERA_DISABLED: camera_status_t = -10012;
    pub const ACAMERA_ERROR_CAMERA_DISCONNECTED: camera_status_t = -10013;

    // -----------------------------------------------------------------------
    // media_status_t constants
    // -----------------------------------------------------------------------

    pub const AMEDIA_OK: media_status_t = 0;
    pub const AMEDIA_ERROR_UNKNOWN: media_status_t = -10000;
    pub const AMEDIA_ERROR_INVALID_PARAM: media_status_t = -10004;

    // -----------------------------------------------------------------------
    // AImage format constants
    // -----------------------------------------------------------------------

    pub const AIMAGE_FORMAT_RGBA_8888: i32 = 0x0000_0001;
    pub const AIMAGE_FORMAT_YUV_420_888: i32 = 0x0000_0023;
    pub const AIMAGE_FORMAT_JPEG: i32 = 0x0000_0100;

    // -----------------------------------------------------------------------
    // Lens facing (from ACAMERA_LENS_FACING tag values)
    // -----------------------------------------------------------------------

    pub const ACAMERA_LENS_FACING_FRONT: u8 = 0;
    pub const ACAMERA_LENS_FACING_BACK: u8 = 1;
    pub const ACAMERA_LENS_FACING_EXTERNAL: u8 = 2;
    /// Metadata tag for lens facing direction.
    pub const ACAMERA_LENS_FACING: u32 = 0x10005;

    // -----------------------------------------------------------------------
    // ACameraDevice capture template constants
    // -----------------------------------------------------------------------

    pub const TEMPLATE_PREVIEW: c_int = 1;
    pub const TEMPLATE_STILL_CAPTURE: c_int = 2;
    pub const TEMPLATE_RECORD: c_int = 3;

    // -----------------------------------------------------------------------
    // AMediaRecorder constants (API 26+)
    // -----------------------------------------------------------------------

    pub const AMEDIARECORDER_OUTPUT_FORMAT_MPEG_4: c_int = 2;
    pub const AMEDIARECORDER_VIDEO_ENCODER_H264: c_int = 2;
    pub const AMEDIARECORDER_VIDEO_SOURCE_SURFACE: c_int = 2;

    // -----------------------------------------------------------------------
    // Opaque NDK types — never dereferenced from Rust
    // -----------------------------------------------------------------------

    #[repr(C)]
    pub struct ACameraManager {
        _priv: [u8; 0],
    }
    #[repr(C)]
    pub struct ACameraDevice {
        _priv: [u8; 0],
    }
    #[repr(C)]
    pub struct ACameraCaptureSession {
        _priv: [u8; 0],
    }
    #[repr(C)]
    pub struct ACameraMetadata {
        _priv: [u8; 0],
    }
    #[repr(C)]
    pub struct ACaptureRequest {
        _priv: [u8; 0],
    }
    #[repr(C)]
    pub struct ACaptureSessionOutput {
        _priv: [u8; 0],
    }
    #[repr(C)]
    pub struct ACaptureSessionOutputContainer {
        _priv: [u8; 0],
    }
    #[repr(C)]
    pub struct ACameraOutputTarget {
        _priv: [u8; 0],
    }
    #[repr(C)]
    pub struct AImageReader {
        _priv: [u8; 0],
    }
    #[repr(C)]
    pub struct AImage {
        _priv: [u8; 0],
    }
    #[repr(C)]
    pub struct ANativeWindow {
        _priv: [u8; 0],
    }
    #[repr(C)]
    pub struct AMediaRecorder {
        _priv: [u8; 0],
    }

    // -----------------------------------------------------------------------
    // ACameraIdList (NdkCameraManager.h)
    // -----------------------------------------------------------------------

    #[repr(C)]
    pub struct ACameraIdList {
        pub numCameras: c_int,
        pub cameraIds: *mut *const c_char,
    }

    // -----------------------------------------------------------------------
    // ACameraMetadata entry (NdkCameraMetadata.h)
    // -----------------------------------------------------------------------

    #[repr(C)]
    pub struct ACameraMetadata_rational {
        pub numerator: i32,
        pub denominator: i32,
    }

    #[repr(C)]
    pub union ACameraMetadata_entry_data {
        pub u8_: *const u8,
        pub i32_: *const i32,
        pub f_: *const f32,
        pub i64_: *const i64,
        pub d_: *const f64,
        pub r_: *const ACameraMetadata_rational,
    }

    #[repr(C)]
    pub struct ACameraMetadata_const_entry {
        pub tag: u32,
        pub type_: u8,
        pub count: u32,
        pub data: ACameraMetadata_entry_data,
    }

    // -----------------------------------------------------------------------
    // Callback types — camera device state (NdkCameraDevice.h)
    // -----------------------------------------------------------------------

    pub type ACameraDevice_StateCallback =
        Option<unsafe extern "C" fn(context: *mut c_void, device: *mut ACameraDevice)>;
    pub type ACameraDevice_ErrorStateCallback = Option<
        unsafe extern "C" fn(context: *mut c_void, device: *mut ACameraDevice, error: c_int),
    >;

    #[repr(C)]
    pub struct ACameraDevice_StateCallbacks {
        pub context: *mut c_void,
        pub onDisconnected: ACameraDevice_StateCallback,
        pub onError: ACameraDevice_ErrorStateCallback,
    }

    // -----------------------------------------------------------------------
    // Callback types — capture session state (NdkCameraCaptureSession.h)
    // -----------------------------------------------------------------------

    pub type ACameraCaptureSession_stateCallback =
        Option<unsafe extern "C" fn(context: *mut c_void, session: *mut ACameraCaptureSession)>;

    #[repr(C)]
    pub struct ACameraCaptureSession_stateCallbacks {
        pub context: *mut c_void,
        pub onClosed: ACameraCaptureSession_stateCallback,
        pub onReady: ACameraCaptureSession_stateCallback,
        pub onActive: ACameraCaptureSession_stateCallback,
    }

    // -----------------------------------------------------------------------
    // Callback types — image reader (NdkImageReader.h)
    // -----------------------------------------------------------------------

    pub type AImageReader_ImageCallback =
        Option<unsafe extern "C" fn(context: *mut c_void, reader: *mut AImageReader)>;

    #[repr(C)]
    pub struct AImageReader_ImageListener {
        pub context: *mut c_void,
        pub onImageAvailable: AImageReader_ImageCallback,
    }

    // -----------------------------------------------------------------------
    // Camera manager functions (NdkCameraManager.h)
    // -----------------------------------------------------------------------

    #[link(name = "camera2ndk")]
    unsafe extern "C" {
        pub fn ACameraManager_create() -> *mut ACameraManager;
        pub fn ACameraManager_delete(manager: *mut ACameraManager);
        pub fn ACameraManager_getCameraIdList(
            manager: *mut ACameraManager,
            cameraIdList: *mut *mut ACameraIdList,
        ) -> camera_status_t;
        pub fn ACameraManager_deleteCameraIdList(cameraIdList: *mut ACameraIdList);
        pub fn ACameraManager_getCameraCharacteristics(
            manager: *mut ACameraManager,
            cameraId: *const c_char,
            characteristics: *mut *mut ACameraMetadata,
        ) -> camera_status_t;
        pub fn ACameraManager_openCamera(
            manager: *mut ACameraManager,
            cameraId: *const c_char,
            callback: *const ACameraDevice_StateCallbacks,
            device: *mut *mut ACameraDevice,
        ) -> camera_status_t;
    }

    // -----------------------------------------------------------------------
    // Camera device functions (NdkCameraDevice.h)
    // -----------------------------------------------------------------------

    unsafe extern "C" {
        pub fn ACameraDevice_close(device: *mut ACameraDevice) -> camera_status_t;
        pub fn ACameraDevice_createCaptureRequest(
            device: *mut ACameraDevice,
            templateId: c_int,
            request: *mut *mut ACaptureRequest,
        ) -> camera_status_t;
        pub fn ACameraDevice_createCaptureSession(
            device: *mut ACameraDevice,
            outputs: *const ACaptureSessionOutputContainer,
            callbacks: *const ACameraCaptureSession_stateCallbacks,
            session: *mut *mut ACameraCaptureSession,
        ) -> camera_status_t;
    }

    // -----------------------------------------------------------------------
    // Capture session / output functions (NdkCameraCaptureSession.h)
    // -----------------------------------------------------------------------

    unsafe extern "C" {
        pub fn ACameraCaptureSession_close(session: *mut ACameraCaptureSession);
        pub fn ACameraCaptureSession_setRepeatingRequest(
            session: *mut ACameraCaptureSession,
            callbacks: *mut c_void,
            numRequests: c_int,
            requests: *mut *mut ACaptureRequest,
            captureSequenceId: *mut c_int,
        ) -> camera_status_t;
        pub fn ACameraCaptureSession_stopRepeating(
            session: *mut ACameraCaptureSession,
        ) -> camera_status_t;

        pub fn ACaptureSessionOutputContainer_create(
            container: *mut *mut ACaptureSessionOutputContainer,
        ) -> camera_status_t;
        pub fn ACaptureSessionOutputContainer_free(container: *mut ACaptureSessionOutputContainer);
        pub fn ACaptureSessionOutput_create(
            target: *mut ANativeWindow,
            output: *mut *mut ACaptureSessionOutput,
        ) -> camera_status_t;
        pub fn ACaptureSessionOutput_free(output: *mut ACaptureSessionOutput);
        pub fn ACaptureSessionOutputContainer_add(
            container: *mut ACaptureSessionOutputContainer,
            output: *const ACaptureSessionOutput,
        ) -> camera_status_t;

        pub fn ACameraOutputTarget_create(
            window: *mut ANativeWindow,
            output: *mut *mut ACameraOutputTarget,
        ) -> camera_status_t;
        pub fn ACameraOutputTarget_free(output: *mut ACameraOutputTarget);

        pub fn ACaptureRequest_addTarget(
            request: *mut ACaptureRequest,
            output: *const ACameraOutputTarget,
        ) -> camera_status_t;
        pub fn ACaptureRequest_removeTarget(
            request: *mut ACaptureRequest,
            output: *const ACameraOutputTarget,
        ) -> camera_status_t;
        pub fn ACaptureRequest_free(request: *mut ACaptureRequest);
    }

    // -----------------------------------------------------------------------
    // Camera metadata functions (NdkCameraMetadata.h)
    // -----------------------------------------------------------------------

    unsafe extern "C" {
        pub fn ACameraMetadata_getConstEntry(
            metadata: *const ACameraMetadata,
            tag: u32,
            entry: *mut ACameraMetadata_const_entry,
        ) -> camera_status_t;
        pub fn ACameraMetadata_free(metadata: *mut ACameraMetadata);
    }

    // -----------------------------------------------------------------------
    // Image reader / image functions (NdkImageReader.h, NdkImage.h)
    // -----------------------------------------------------------------------

    #[link(name = "mediandk")]
    unsafe extern "C" {
        pub fn AImageReader_new(
            width: i32,
            height: i32,
            format: i32,
            maxImages: i32,
            reader: *mut *mut AImageReader,
        ) -> media_status_t;
        pub fn AImageReader_delete(reader: *mut AImageReader);
        pub fn AImageReader_getWindow(
            reader: *mut AImageReader,
            window: *mut *mut ANativeWindow,
        ) -> media_status_t;
        pub fn AImageReader_acquireLatestImage(
            reader: *mut AImageReader,
            image: *mut *mut AImage,
        ) -> media_status_t;
        pub fn AImageReader_setImageListener(
            reader: *mut AImageReader,
            listener: *mut AImageReader_ImageListener,
        ) -> media_status_t;

        pub fn AImage_delete(image: *mut AImage);
        pub fn AImage_getWidth(image: *const AImage, width: *mut i32) -> media_status_t;
        pub fn AImage_getHeight(image: *const AImage, height: *mut i32) -> media_status_t;
        pub fn AImage_getTimestamp(image: *const AImage, timestampNs: *mut i64) -> media_status_t;
        pub fn AImage_getNumberOfPlanes(
            image: *const AImage,
            numPlanes: *mut i32,
        ) -> media_status_t;
        pub fn AImage_getPlaneData(
            image: *const AImage,
            planeIdx: i32,
            data: *mut *const u8,
            dataLength: *mut i32,
        ) -> media_status_t;
        pub fn AImage_getPlanePixelStride(
            image: *const AImage,
            planeIdx: i32,
            pixelStride: *mut i32,
        ) -> media_status_t;
        pub fn AImage_getPlaneRowStride(
            image: *const AImage,
            planeIdx: i32,
            rowStride: *mut i32,
        ) -> media_status_t;
    }

    // -----------------------------------------------------------------------
    // AMediaRecorder functions (NdkMediaRecorder.h — API 26+)
    // -----------------------------------------------------------------------

    unsafe extern "C" {
        pub fn AMediaRecorder_new() -> *mut AMediaRecorder;
        pub fn AMediaRecorder_delete(recorder: *mut AMediaRecorder) -> media_status_t;
        pub fn AMediaRecorder_setVideoSource(
            recorder: *mut AMediaRecorder,
            videoSource: c_int,
        ) -> media_status_t;
        pub fn AMediaRecorder_setOutputFormat(
            recorder: *mut AMediaRecorder,
            format: c_int,
        ) -> media_status_t;
        pub fn AMediaRecorder_setVideoEncoder(
            recorder: *mut AMediaRecorder,
            encoder: c_int,
        ) -> media_status_t;
        pub fn AMediaRecorder_setVideoSize(
            recorder: *mut AMediaRecorder,
            width: c_int,
            height: c_int,
        ) -> media_status_t;
        pub fn AMediaRecorder_setVideoFrameRate(
            recorder: *mut AMediaRecorder,
            rate: c_int,
        ) -> media_status_t;
        pub fn AMediaRecorder_setVideoEncodingBitRate(
            recorder: *mut AMediaRecorder,
            bitRate: c_int,
        ) -> media_status_t;
        pub fn AMediaRecorder_setOutputFilePath(
            recorder: *mut AMediaRecorder,
            path: *const c_char,
        ) -> media_status_t;
        pub fn AMediaRecorder_prepare(recorder: *mut AMediaRecorder) -> media_status_t;
        pub fn AMediaRecorder_start(recorder: *mut AMediaRecorder) -> media_status_t;
        pub fn AMediaRecorder_stop(recorder: *mut AMediaRecorder) -> media_status_t;
        /// Returns the recorder's input surface (API 26+). The caller does NOT
        /// own the returned window; it is owned by the recorder.
        pub fn AMediaRecorder_getInputSurface(recorder: *mut AMediaRecorder) -> *mut ANativeWindow;
    }
}

// When the `bindgen` feature is enabled, generated bindings override the
// hand-written ones. Build with `--features bindgen` and `ANDROID_NDK_ROOT`
// set to regenerate.
#[cfg(all(target_os = "android", feature = "bindgen"))]
include!(concat!(env!("OUT_DIR"), "/camera2_bindings.rs"));
