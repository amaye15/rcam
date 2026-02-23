# rcam-sys-android

Raw `unsafe` Rust FFI declarations for the Android **Camera2 NDK** and **AMediaRecorder** APIs.

This crate is a workspace member of [`rcam`](../README.md).  Most users should depend on `rcam` directly — this crate is only useful if you want to call Camera2 NDK functions without the high-level abstraction layer.

## What's included

- `ACameraManager` — device enumeration
- `ACameraDevice` — open / close a physical camera
- `ACameraCaptureSession` — repeating capture sessions
- `ACaptureRequest` / `ACaptureSessionOutput` / `ACameraOutputTarget`
- `AImageReader` / `AImage` — frame delivery
- `ACameraMetadata` — capability and characteristic queries
- `AMediaRecorder` (API 26+) — H.264 / MP4 recording via surface input

## Minimum Android API level

- **24** (Android 7.0) — Camera2 NDK core
- **26** (Android 8.0) — `AMediaRecorder_getInputSurface`

## Hand-written vs generated bindings

By default this crate ships hand-written FFI declarations that match NDK r26+.  Enable the `bindgen` feature (and set `ANDROID_NDK_ROOT`) to regenerate them from the real NDK headers:

```sh
ANDROID_NDK_ROOT=/path/to/ndk \
  cargo build --target aarch64-linux-android --features bindgen
```

## License

Licensed under either of [Apache 2.0](../LICENSE-APACHE) or [MIT](../LICENSE-MIT) at your option.
