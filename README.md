# rcam

A unified, async Rust API for camera capture and video recording across **iOS, macOS, Android, Linux, Windows, and WebAssembly**.

The correct native backend is selected automatically at compile time — the same pattern used by [`cpal`](https://github.com/RustAudio/cpal) for audio and [`winit`](https://github.com/rust-windowing/winit) for windowing.  You import `rcam::Camera` and write one set of code for every target.

## Platform support

| Platform | Backend | Status |
|----------|---------|--------|
| macOS | AVFoundation | ✅ |
| iOS | AVFoundation | ✅ |
| Linux | V4L2 | ✅ |
| Windows | Media Foundation | ✅ |
| Android | Camera2 NDK | ✅ |
| WASM | `getUserMedia` / `MediaRecorder` | ✅ |

## Quick start

```toml
[dependencies]
rcam = "0.1"
tokio = { version = "1", features = ["full"] }
```

```rust
use rcam::{Camera, CameraConfig, CameraDevice, RecordingOutput};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // List cameras
    for cam in Camera::enumerate().await? {
        println!("{} ({:?})", cam.name, cam.position);
    }

    // Open the default camera at 720p 30 fps
    let mut cam = Camera::open(CameraConfig::default()).await?;
    cam.start_stream().await?;

    // Grab a still image
    let frame = cam.take_photo().await?;
    println!("{}×{} {:?}", frame.width, frame.height, frame.format);

    // Record five seconds of video
    cam.start_recording(RecordingOutput::File("out.mp4".into())).await?;
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    let video = cam.stop_recording().await?;

    cam.stop_stream().await?;
    cam.close().await?;
    Ok(())
}
```

## Optional features

| Feature | What it enables |
|---------|----------------|
| `serde` | `Serialize` / `Deserialize` on `Frame`, `CameraInfo`, `Resolution`, etc. |
| `image-output` | `Frame::to_image()` → `image::DynamicImage` (supports YUV420, NV12, BGRA, RGB24, MJPEG) |

Enable in `Cargo.toml`:

```toml
rcam = { version = "0.1", features = ["serde", "image-output"] }
```

## Examples

```sh
cargo run --example list_devices
cargo run --example snapshot --features image-output
cargo run --example record
```

## Android setup

Camera2 NDK requires **Android API 24+**.  Add permissions to your `AndroidManifest.xml`:

```xml
<uses-permission android:name="android.permission.CAMERA" />
<uses-permission android:name="android.permission.RECORD_AUDIO" />
```

Runtime permission must be granted from the Java/Kotlin layer before calling any `rcam` API.

## Crate layout

| Crate | Purpose |
|-------|---------|
| `rcam` | Main public-facing crate — the only one most users need |
| `rcam-sys-android` | Raw `unsafe` Camera2 NDK + AMediaRecorder FFI (hand-written, with optional `bindgen` regeneration) |

## License

Licensed under either of [Apache 2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT) at your option.
