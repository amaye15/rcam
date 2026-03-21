# Cross-Platform Rust Camera Crate — Complete Plan

## Overview

A unified Rust crate (`rcam`) providing a single, ergonomic API for camera capture and video recording across **iOS, macOS, Android, Linux, Windows, and Web (WASM)**. The crate auto-selects the correct native backend at compile time using `#[cfg(...)]` conditional compilation — the same pattern used by `cpal` (audio) and `winit` (windowing).

**What exists today and why this crate is needed:**
- `nokhwa` — desktop-only (Linux/macOS/Windows), no mobile, no WASM
- `tauri-plugin-camera` — Android-only, tied to Tauri, no standalone use
- `objc2-av-foundation` — iOS/macOS bindings exist but no safe camera abstraction layer
- Android Camera2 NDK — no safe Rust bindings exist yet, must be handwritten

---

## Crate Architecture

### Workspace Structure

```
rcam/
├── Cargo.toml                    # workspace root
├── rcam/                         # main public-facing crate
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs                # re-exports, #[cfg] backend selection
│       ├── traits.rs             # CameraDevice trait + async API
│       ├── types.rs              # shared types: Frame, VideoData, Resolution, etc.
│       ├── error.rs              # unified CameraError enum
│       ├── config.rs             # CameraConfig, FrameFormat, codec options
│       └── backend/
│           ├── mod.rs            # cfg dispatch
│           ├── avfoundation/     # iOS + macOS
│           │   ├── mod.rs
│           │   ├── session.rs    # AVCaptureSession management
│           │   ├── device.rs     # AVCaptureDevice enumeration
│           │   └── recorder.rs   # AVAssetWriter for video recording
│           ├── android/          # Android
│           │   ├── mod.rs
│           │   ├── camera2.rs    # Camera2 NDK via bindgen
│           │   ├── bindings.rs   # raw bindgen output (auto-generated)
│           │   └── media.rs      # AMediaRecorder / AMediaMuxer
│           ├── v4l2/             # Linux
│           │   ├── mod.rs
│           │   └── capture.rs    # via `v4l` crate
│           ├── mediafoundation/  # Windows
│           │   ├── mod.rs
│           │   └── capture.rs    # via `windows-rs` MF APIs
│           └── web/              # WASM
│               ├── mod.rs
│               ├── capture.rs    # navigator.mediaDevices.getUserMedia
│               └── recorder.rs   # MediaRecorder API
├── rcam-sys-android/             # raw Camera2 NDK sys bindings (re-usable)
│   ├── Cargo.toml
│   ├── build.rs                  # bindgen against NDK headers
│   └── src/lib.rs
├── examples/
│   ├── snapshot.rs               # take a photo, save to disk
│   ├── record.rs                 # record 5 seconds of video
│   ├── list_devices.rs           # enumerate cameras
│   └── web/                      # wasm-pack demo
│       └── index.html
└── tests/
    ├── integration/
    └── mock_backend/             # testable mock for CI without hardware
```

---

## Public API Design

### Core Trait

```rust
// rcam/src/traits.rs
use crate::{CameraConfig, CameraError, CameraInfo, Frame, VideoData};

pub trait CameraDevice: Send + Sync {
    /// List all available cameras on the system
    async fn enumerate() -> Result<Vec<CameraInfo>, CameraError>;

    /// Open a camera by index or ID
    async fn open(config: CameraConfig) -> Result<Self, CameraError>
    where
        Self: Sized;

    /// Start continuous frame capture; returns a stream of frames
    async fn start_stream(&mut self) -> Result<(), CameraError>;

    /// Grab a single frame from the live stream
    async fn capture_frame(&self) -> Result<Frame, CameraError>;

    /// Take a single still image (highest quality)
    async fn take_photo(&self) -> Result<Frame, CameraError>;

    /// Begin video recording to an internal buffer or file path
    async fn start_recording(&mut self, output: RecordingOutput) -> Result<(), CameraError>;

    /// Stop recording and return the final video data/path
    async fn stop_recording(&mut self) -> Result<VideoData, CameraError>;

    /// Stop the frame stream
    async fn stop_stream(&mut self) -> Result<(), CameraError>;

    /// Query device capabilities (resolution, frame rates, formats)
    fn capabilities(&self) -> &CameraCapabilities;

    /// Close the device and release hardware resources
    async fn close(self) -> Result<(), CameraError>;
}
```

### Shared Types

```rust
// rcam/src/types.rs

pub struct CameraInfo {
    pub id: String,
    pub name: String,
    pub position: CameraPosition,   // Front, Back, External, Unknown
    pub is_default: bool,
}

pub struct CameraConfig {
    pub device_id: Option<String>,  // None = system default
    pub resolution: Resolution,
    pub frame_rate: u32,
    pub format: FrameFormat,        // MJPEG, NV12, YUV420, BGRA
    pub position: CameraPosition,
}

pub struct Frame {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub format: FrameFormat,
    pub timestamp_us: u64,
}

pub struct VideoData {
    pub kind: VideoOutput,
}

pub enum VideoOutput {
    File(PathBuf),           // recorded to disk
    Buffer(Vec<u8>),         // in-memory (WASM / small clips)
}

pub struct Resolution {
    pub width: u32,
    pub height: u32,
}

pub enum CameraPosition {
    Front,
    Back,
    External,
    Unknown,
}

pub enum FrameFormat {
    MJPEG,
    NV12,
    YUV420,
    BGRA,
    RGB24,
}

pub enum RecordingOutput {
    File(PathBuf),
    Buffer,   // in-memory, mainly for web
}

pub struct CameraCapabilities {
    pub supported_resolutions: Vec<Resolution>,
    pub supported_frame_rates: Vec<u32>,
    pub supported_formats: Vec<FrameFormat>,
    pub has_torch: bool,
    pub has_zoom: bool,
}
```

### Error Type

```rust
// rcam/src/error.rs
#[derive(Debug, thiserror::Error)]
pub enum CameraError {
    #[error("No camera device found")]
    NoCameraFound,
    #[error("Permission denied")]
    PermissionDenied,
    #[error("Device already in use")]
    DeviceBusy,
    #[error("Unsupported format: {0:?}")]
    UnsupportedFormat(FrameFormat),
    #[error("Recording not started")]
    NotRecording,
    #[error("Stream not active")]
    StreamNotActive,
    #[error("Backend error: {0}")]
    Backend(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Platform not supported")]
    Unsupported,
}
```

### Top-Level Entry Point

```rust
// rcam/src/lib.rs

// Compile-time backend selection
#[cfg(any(target_os = "ios", target_os = "macos"))]
use backend::avfoundation::AvfCamera as PlatformCamera;

#[cfg(target_os = "android")]
use backend::android::AndroidCamera as PlatformCamera;

#[cfg(target_os = "linux")]
use backend::v4l2::V4l2Camera as PlatformCamera;

#[cfg(target_os = "windows")]
use backend::mediafoundation::MfCamera as PlatformCamera;

#[cfg(target_arch = "wasm32")]
use backend::web::WebCamera as PlatformCamera;

/// Convenience alias — users just use `Camera`, not platform-specific types
pub type Camera = PlatformCamera;
```

---

## Backend Implementation Plans

### 1. iOS & macOS — AVFoundation

**Crates:** `objc2-av-foundation`, `objc2-foundation`, `objc2-core-media`, `objc2-core-video`

**Key AVFoundation classes needed:**
- `AVCaptureSession` — orchestrates the entire capture pipeline
- `AVCaptureDevice` — represents a physical camera, used for enumeration
- `AVCaptureDeviceInput` — wraps a device for use in a session
- `AVCaptureVideoDataOutput` — delivers raw frames via a delegate callback
- `AVCapturePhotoOutput` — still photo capture
- `AVAssetWriter` + `AVAssetWriterInput` — video recording to file
- `AVCaptureMovieFileOutput` — simpler video recording alternative

**Known gotcha:** `objc2-av-foundation` exposes the full Obj-C API but all calls are `unsafe`. Need to wrap in safe Rust abstractions. Delegate callbacks (for frame delivery) require implementing `AVCaptureVideoDataOutputSampleBufferDelegate` via `define_class!` macro.

**Implementation steps:**
1. Enumerate cameras via `AVCaptureDevice::devices_with_media_type()`
2. Create `AVCaptureSession`, set `sessionPreset`
3. Add `AVCaptureDeviceInput` to session
4. Add `AVCaptureVideoDataOutput` with a Rust delegate that receives `CMSampleBuffer`
5. Convert `CMSampleBuffer` → `CVPixelBuffer` → raw bytes for `Frame`
6. For recording: add `AVAssetWriter` writing to `.mov`/`.mp4`

**Feature flag:** `backend-avfoundation` (auto-enabled on iOS/macOS)

---

### 2. Android — Camera2 NDK

**The gap:** The `ndk` crate from rust-mobile explicitly does not cover Camera2 NDK. Must write `rcam-sys-android` from scratch.

**NDK headers to bind (via `bindgen` in `build.rs`):**
```
NdkCameraManager.h     → ACameraManager, device enumeration
NdkCameraDevice.h      → ACameraDevice, open/close
NdkCameraCaptureSession.h → ACameraCaptureSession, repeating capture
NdkCameraMetadata.h    → ACameraMetadata, capability queries
NdkImage.h             → AImage, raw frame access
NdkImageReader.h       → AImageReader, frame queue
NdkMediaRecorder.h     → AMediaRecorder, video recording
NdkMediaMuxer.h        → AMediaMuxer, MP4 muxing
```

**NDK minimum API level:** 24 (Android 7.0) for Camera2 NDK — this covers 97%+ of devices.

**Implementation steps:**
1. `build.rs` generates bindings from NDK headers found via `ANDROID_NDK_ROOT` env var
2. Link `libcamera2ndk.so`, `libmediandk.so`, `libandroid.so`
3. `ACameraManager_create()` → enumerate devices
4. `ACameraManager_openCamera()` → get `ACameraDevice`
5. `AImageReader_new()` → create frame output surface
6. `ACameraDevice_createCaptureSession()` → start repeating capture
7. `AImageReader_getNextImage()` → get frames as `AImage`, read planes into `Frame`
8. `AMediaRecorder` for video recording

**Permissions:** Must be declared in `AndroidManifest.xml` — document clearly for users:
```xml
<uses-permission android:name="android.permission.CAMERA"/>
<uses-permission android:name="android.permission.RECORD_AUDIO"/>
<uses-permission android:name="android.permission.WRITE_EXTERNAL_STORAGE"/>
```

**Feature flag:** `backend-android` (auto-enabled on Android)

---

### 3. Linux — V4L2

**Crate:** `v4l` (Video4Linux2 bindings, actively maintained, ~150K downloads)

**What `v4l` provides:**
- Device enumeration (`v4l::Device::with_path`)
- Format negotiation (MJPEG, YUYV, NV12)
- Streaming capture via `MmapStream`
- Frame buffer access

**Implementation steps:**
1. Enumerate `/dev/video*` via `v4l::query_devices()`
2. Open device, negotiate format via `v4l::Format`
3. Create `MmapStream`, loop calling `stream.next()`
4. Copy buffer data into `Frame`
5. For video recording: pipe frames into `ffmpeg-next` or write raw to file

**Note:** Linux has no native video recording API — encoding is handled separately (ffmpeg-next recommended as optional dep).

**Feature flag:** `backend-v4l2` (auto-enabled on Linux)

---

### 4. Windows — Media Foundation

**Crate:** `windows-rs` with `Media_Capture` and `Media_MediaProperties` features

**Key Windows MF types:**
- `MediaCapture` — main entry point
- `MediaCaptureInitializationSettings` — configure device
- `LowLagPhotoCapture` — still photo
- `LowLagMediaRecording` — video recording
- `VideoDeviceController` — resolution/format control

**Implementation steps:**
1. `DeviceInformation::FindAllAsync()` to enumerate cameras
2. `MediaCapture::InitializeAsync()` with chosen device
3. `MediaCapture::StartPreviewAsync()` for frame streaming
4. `MediaCapture::CapturePhotoToStorageFileAsync()` for photos
5. `MediaCapture::StartRecordToStorageFileAsync()` for video

**Note:** Windows MF APIs are `async` by nature via WinRT — maps naturally to Rust async.

**Feature flag:** `backend-mediafoundation` (auto-enabled on Windows)

---

### 5. Web — WASM (getUserMedia + MediaRecorder)

**Crates:** `web-sys`, `js-sys`, `wasm-bindgen`, `wasm-bindgen-futures`

**Browser APIs used:**
- `navigator.mediaDevices.getUserMedia({ video: true })` → `MediaStream`
- `navigator.mediaDevices.enumerateDevices()` → device list
- `MediaRecorder` → video recording to `Blob`
- `HTMLVideoElement` → optional preview surface
- `ImageCapture` → still photo from stream

**Key `web-sys` features needed in Cargo.toml:**
```toml
[target.'cfg(target_arch = "wasm32")'.dependencies.web-sys]
version = "0.3"
features = [
  "MediaDevices", "MediaStream", "MediaStreamConstraints",
  "MediaRecorder", "MediaRecorderOptions", "BlobEvent",
  "ImageCapture", "MediaStreamTrack", "Navigator",
  "Window", "HtmlVideoElement", "InputDeviceInfo",
]
```

**Implementation steps:**
1. `window.navigator().media_devices()` to get `MediaDevices`
2. `JsFuture::from(devices.get_user_media_with_constraints())` → `MediaStream`
3. Frame capture: draw stream to `<canvas>`, read pixel data via `getImageData()`
4. Recording: `MediaRecorder::new_with_media_stream()`, collect `dataavailable` events into `Vec<u8>`
5. On stop: combine blobs → `Buffer` variant of `VideoOutput`

**WASM-specific considerations:**
- All ops are async (Promises) — use `wasm-bindgen-futures::JsFuture`
- No file system access — recording always returns `VideoOutput::Buffer`
- Permission prompt is browser-native, no Rust intervention needed
- Camera enumeration only works after `getUserMedia` has been granted

**Feature flag:** `backend-web` (auto-enabled on `wasm32`)

---

## Cargo.toml Feature Flags

```toml
[features]
default = []

# Automatically selected by build — users don't need to set these manually
backend-avfoundation = ["dep:objc2", "dep:objc2-av-foundation", "dep:objc2-foundation",
                         "dep:objc2-core-media", "dep:objc2-core-video"]
backend-android      = ["dep:rcam-sys-android", "dep:jni"]
backend-v4l2         = ["dep:v4l"]
backend-mf           = ["dep:windows"]
backend-web          = ["dep:web-sys", "dep:js-sys", "dep:wasm-bindgen",
                         "dep:wasm-bindgen-futures"]

# Optional capabilities
ffmpeg-encoding      = ["dep:ffmpeg-next"]  # Linux/Android encoding
serde                = ["dep:serde"]         # Serialize Frame, CameraInfo etc.
image-output         = ["dep:image"]         # Convert Frame to image::DynamicImage

[target.'cfg(any(target_os = "ios", target_os = "macos"))'.dependencies]
objc2 = "0.5"
objc2-av-foundation = { version = "0.3", features = [
  "AVCaptureSession", "AVCaptureDevice", "AVCaptureDeviceInput",
  "AVCaptureVideoDataOutput", "AVCapturePhotoOutput",
  "AVAssetWriter", "AVAssetWriterInput", "AVCaptureMovieFileOutput"
]}
objc2-foundation = "0.2"
objc2-core-media = "0.2"
objc2-core-video = "0.2"

[target.'cfg(target_os = "android")'.dependencies]
rcam-sys-android = { path = "../rcam-sys-android" }
jni = "0.21"

[target.'cfg(target_os = "linux")'.dependencies]
v4l = "0.14"

[target.'cfg(target_os = "windows")'.dependencies.windows]
version = "0.58"
features = [
  "Media_Capture", "Media_MediaProperties",
  "Devices_Enumeration", "Foundation_Collections"
]

[target.'cfg(target_arch = "wasm32")'.dependencies]
web-sys = { version = "0.3", features = [...] }
js-sys = "0.3"
wasm-bindgen = "0.2"
wasm-bindgen-futures = "0.4"
```

---

## Build Script (build.rs for Android)

```rust
// rcam-sys-android/build.rs
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() != "android" {
        return;
    }

    let ndk = std::env::var("ANDROID_NDK_ROOT")
        .or_else(|_| std::env::var("ANDROID_NDK_HOME"))
        .expect("ANDROID_NDK_ROOT must be set for Android builds");

    let api_level = "24"; // minimum for Camera2 NDK
    let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap();

    let sysroot = format!(
        "{}/toolchains/llvm/prebuilt/linux-x86_64/sysroot",
        ndk
    );
    let include = format!("{}/usr/include", sysroot);

    // Link the camera and media NDK libraries
    println!("cargo:rustc-link-lib=camera2ndk");
    println!("cargo:rustc-link-lib=mediandk");
    println!("cargo:rustc-link-lib=android");

    let bindings = bindgen::Builder::default()
        .header(format!("{}/camera/NdkCameraManager.h", include))
        .header(format!("{}/camera/NdkCameraDevice.h", include))
        .header(format!("{}/camera/NdkCameraCaptureSession.h", include))
        .header(format!("{}/camera/NdkCameraMetadata.h", include))
        .header(format!("{}/camera/NdkCameraMetadataTags.h", include))
        .header(format!("{}/media/NdkImage.h", include))
        .header(format!("{}/media/NdkImageReader.h", include))
        .header(format!("{}/media/NdkMediaRecorder.h", include))
        .header(format!("{}/media/NdkMediaMuxer.h", include))
        .clang_arg(format!("--sysroot={}", sysroot))
        .clang_arg(format!("-isystem{}", include))
        .allowlist_function("ACamera.*")
        .allowlist_function("AImage.*")
        .allowlist_function("AMediaRecorder.*")
        .allowlist_function("AMediaMuxer.*")
        .allowlist_type("ACamera.*")
        .allowlist_type("AImage.*")
        .allowlist_var("ACAMERA.*")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate()
        .expect("Unable to generate Camera2 NDK bindings");

    let out_path = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("camera2_bindings.rs"))
        .expect("Failed to write bindings");
}
```

---

## Async Runtime Compatibility

The crate uses `async fn` throughout. Platform considerations:

| Platform | Runtime | Notes |
|----------|---------|-------|
| iOS/macOS | tokio or async-std | User's choice |
| Android | tokio | Camera callbacks dispatch to Rust via JNI |
| Linux | tokio or async-std | V4L2 is sync — wrapped in `spawn_blocking` |
| Windows | Windows async (WinRT) | Mapped to futures via `windows-futures` |
| WASM | `wasm-bindgen-futures` | JS Promises → Rust futures |

**Strategy:** use `async-trait` for the `CameraDevice` trait. Keep the trait itself runtime-agnostic. Document that WASM requires a WASM-compatible executor (no `tokio` on WASM).

---

## Testing Strategy

**The challenge:** camera tests require hardware. Solution: a mock backend for CI.

```rust
// tests/mock_backend/mod.rs
pub struct MockCamera {
    frame_counter: u64,
}

impl CameraDevice for MockCamera {
    async fn enumerate() -> Result<Vec<CameraInfo>, CameraError> {
        Ok(vec![CameraInfo {
            id: "mock-0".to_string(),
            name: "Mock Camera".to_string(),
            position: CameraPosition::Unknown,
            is_default: true,
        }])
    }

    async fn capture_frame(&self) -> Result<Frame, CameraError> {
        // Return a synthetic BGRA frame (checkerboard pattern)
        let data = vec![128u8; 640 * 480 * 4];
        Ok(Frame { data, width: 640, height: 480,
                   format: FrameFormat::BGRA, timestamp_us: 0 })
    }
    // ... etc
}
```

**CI matrix:**
- Linux: run real V4L2 tests using a virtual v4l2loopback device
- macOS: run AVFoundation tests in macOS runner with virtual camera
- WASM: run web backend tests via headless browser (playwright)
- Windows: Media Foundation tests in Windows runner
- Android/iOS: snapshot tests only (no hardware in CI) — use mock backend

---

## Implementation Phases & Milestones

### Phase 1 — Foundation (Weeks 1–2)
- Set up workspace with all crates
- Define all public types, traits, error enum
- Implement mock backend for testing
- Set up CI (GitHub Actions) with platform matrix
- Publish `rcam-core` with just types and traits

### Phase 2 — Desktop Backends (Weeks 3–5)
- Linux V4L2 backend (easiest — `v4l` crate is mature)
- macOS AVFoundation backend using `objc2-av-foundation`
- Windows Media Foundation backend using `windows-rs`
- Photo capture working on all three
- Video recording working on all three

### Phase 3 — Web Backend (Week 6)
- WASM `getUserMedia` stream capture
- `MediaRecorder` video recording
- Buffer-based video output
- `wasm-pack` example app

### Phase 4 — iOS Backend (Week 7)
- Reuse AVFoundation code from Phase 2
- Permission handling for iOS (different from macOS)
- Test on real device (simulator has camera limitations)

### Phase 5 — Android Backend (Weeks 8–10)
- Write `rcam-sys-android` bindgen bindings
- Implement Camera2 NDK frame capture
- `AMediaRecorder` video recording
- Permission manifest documentation
- Test on physical Android device (emulator camera support is limited)

### Phase 6 — Polish (Weeks 11–12)
- Unified error messages across backends
- Optional `ffmpeg-next` integration for Linux/Android encoding
- `serde` feature for serializable types
- `image` crate integration for `Frame → DynamicImage`
- Full documentation and examples
- Publish to crates.io

---

## Key Dependencies Summary

| Crate | Purpose | Platforms |
|-------|---------|-----------|
| `objc2-av-foundation` | AVFoundation bindings | iOS, macOS |
| `objc2-foundation` | Foundation framework | iOS, macOS |
| `objc2-core-media` | CMSampleBuffer access | iOS, macOS |
| `objc2-core-video` | CVPixelBuffer access | iOS, macOS |
| `v4l` | Video4Linux2 capture | Linux |
| `windows-rs` | Media Foundation | Windows |
| `web-sys` | Browser camera APIs | WASM |
| `wasm-bindgen-futures` | JS Promise → Future | WASM |
| `bindgen` | Camera2 NDK bindings | Android (build) |
| `jni` | JNI interop (permissions) | Android |
| `async-trait` | Async trait support | All |
| `thiserror` | Error derive macro | All |
| `ffmpeg-next` (opt) | Video encoding | Linux, Android |
| `image` (opt) | Frame conversion | All |
| `serde` (opt) | Serialization | All |

---

## What Differentiates This From nokhwa

| Feature | `nokhwa` | `rcam` (this crate) |
|---------|----------|---------------------|
| Linux | ✅ | ✅ |
| macOS | ✅ | ✅ |
| Windows | ✅ | ✅ |
| iOS | ❌ | ✅ |
| Android | ❌ | ✅ |
| Web/WASM | Partial (broken) | ✅ |
| Async API | ❌ (sync) | ✅ (fully async) |
| Video recording | ❌ | ✅ |
| Active mobile dev | ❌ | ✅ |

---

## Open Questions to Resolve

1. **Android permissions at runtime** — Camera2 NDK requires runtime permissions granted from Java/Kotlin side. Need to decide: document as a user responsibility, or provide a companion Kotlin helper via JNI?

2. **Frame streaming API** — `async fn capture_frame()` (pull-based) vs. a push-based channel/stream. Pull is simpler but pull-based at 30fps from an async loop may have latency. Consider `async_stream` or a `tokio::sync::broadcast` channel.

3. **FFmpeg encoding as optional dep** — On Linux and Android there's no built-in video container muxer. Make `ffmpeg-next` required on those platforms, or document that users bring their own encoder?

4. **Minimum Android API level** — Camera2 NDK requires API 24. Document clearly. Consider a JNI fallback for API 21–23 (Camera2 Java API) in a later phase.

5. **WASM frame capture** — `ImageCapture` API is not universally supported. Canvas-based fallback needed for Firefox.
