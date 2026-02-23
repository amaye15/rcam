//! V4L2 device enumeration and frame capture — Phase 2 implementation.
//!
//! Enumeration: walks `/dev/video*` via `v4l::context::enum_devices()` and
//! filters to actual capture-capable devices via `VIDIOC_QUERYCAP`.
//!
//! Streaming: negotiates YUYV (most webcams) then MJPG; converts YUYV→BGRA
//! in software. Runs a blocking capture loop on a dedicated thread pool task.

use std::path::PathBuf;

use tokio::sync::mpsc;
use v4l::buffer::Type;
use v4l::context;
use v4l::io::mmap::Stream;
use v4l::io::traits::CaptureStream;
use v4l::video::Capture;
use v4l::{Device, FourCC, Fraction};

use crate::{CameraConfig, CameraError, CameraInfo, CameraPosition, Frame, FrameFormat};

// ---------------------------------------------------------------------------
// Enumeration
// ---------------------------------------------------------------------------

/// Enumerate all V4L2 video-capture-capable devices on the system.
pub fn enumerate_devices() -> Result<Vec<CameraInfo>, CameraError> {
    let mut infos = Vec::new();
    for node in context::enum_devices() {
        // Brief open to read driver capabilities; skip unpermitted or
        // non-capture devices (output-only, mem-to-mem, etc.).
        let dev = match Device::with_path(node.path()) {
            Ok(d) => d,
            Err(_) => continue,
        };
        let caps = match dev.query_caps() {
            Ok(c) => c,
            Err(_) => continue,
        };
        use v4l::capability::Flags;
        if !caps.capabilities.contains(Flags::VIDEO_CAPTURE) {
            continue;
        }
        let idx = node.index();
        infos.push(CameraInfo {
            id: idx.to_string(),
            name: caps.card,
            position: CameraPosition::Unknown,
            is_default: idx == 0,
        });
    }
    Ok(infos)
}

// ---------------------------------------------------------------------------
// Device handle — carries negotiated format across threads
// ---------------------------------------------------------------------------

/// Encapsulates the resolved device path and negotiated stream parameters.
pub struct V4l2Handle {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    /// Raw FourCC bytes, e.g. `*b"YUYV"` or `*b"MJPG"`.
    pub fourcc: [u8; 4],
}

/// Open and configure a V4L2 device per `config`.
///
/// Tries YUYV first (widest hardware support), then MJPG. Checks whether the
/// driver actually accepted the requested format by inspecting the returned
/// FourCC from `VIDIOC_S_FMT`.
pub fn open_device(config: &CameraConfig) -> Result<V4l2Handle, CameraError> {
    let path = match &config.device_id {
        Some(id) => {
            let idx: usize = id.parse().map_err(|_| {
                CameraError::Backend(format!(
                    "invalid device id '{id}': expected an integer index"
                ))
            })?;
            PathBuf::from(format!("/dev/video{idx}"))
        }
        None => PathBuf::from("/dev/video0"),
    };

    let dev = Device::with_path(&path)
        .map_err(|e| CameraError::Backend(format!("open {}: {e}", path.display())))?;

    let w = config.resolution.width;
    let h = config.resolution.height;

    // Negotiate pixel format — YUYV is supported by nearly all USB webcams.
    // MJPG is a common fallback and saves USB bandwidth.
    let mut negotiated: Option<[u8; 4]> = None;
    for &candidate in &[*b"YUYV", *b"MJPG"] {
        let mut fmt = dev
            .format()
            .map_err(|e| CameraError::Backend(e.to_string()))?;
        fmt.width = w;
        fmt.height = h;
        fmt.fourcc = FourCC { repr: candidate };
        match dev.set_format(&fmt) {
            Ok(actual) if actual.fourcc.repr == candidate => {
                negotiated = Some(candidate);
                break;
            }
            _ => {}
        }
    }

    let fourcc = negotiated.ok_or_else(|| {
        CameraError::Backend("no supported pixel format found (tried YUYV, MJPG)".into())
    })?;

    // Apply requested frame rate (best-effort; not all cameras honour it).
    let _ = dev.set_params(&v4l::video::capture::Parameters::new(Fraction::new(
        1,
        config.frame_rate,
    )));

    Ok(V4l2Handle {
        path,
        width: w,
        height: h,
        fourcc,
    })
}

// ---------------------------------------------------------------------------
// Blocking capture loop
// ---------------------------------------------------------------------------

/// Blocking capture loop — run this inside `tokio::task::spawn_blocking`.
///
/// Opens the device, applies the format negotiated by [`open_device`], creates
/// an `MmapStream`, and continuously calls `stream.next()`, converting each
/// buffer into a [`Frame`] and sending it through `tx`.
///
/// Exits when `tx` is dropped (receiver hung up) or on stream error.
pub fn capture_loop(handle: V4l2Handle, tx: mpsc::UnboundedSender<Frame>) {
    let dev = match Device::with_path(&handle.path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!(
                "rcam/v4l2: failed to reopen device {}: {e}",
                handle.path.display()
            );
            return;
        }
    };

    // Re-apply the negotiated format for this second open of the fd.
    {
        let mut fmt = match dev.format() {
            Ok(f) => f,
            Err(e) => {
                eprintln!("rcam/v4l2: VIDIOC_G_FMT failed: {e}");
                return;
            }
        };
        fmt.width = handle.width;
        fmt.height = handle.height;
        fmt.fourcc = FourCC {
            repr: handle.fourcc,
        };
        let _ = dev.set_format(&fmt);
    }

    let mut stream = match Stream::with_buffers(&dev, Type::VideoCapture, 4) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("rcam/v4l2: stream init failed: {e}");
            return;
        }
    };

    let is_yuyv = handle.fourcc == *b"YUYV";
    let w = handle.width;
    let h = handle.height;

    loop {
        let (buf, meta) = match stream.next() {
            Ok(x) => x,
            Err(e) => {
                eprintln!("rcam/v4l2: stream error: {e}");
                break;
            }
        };

        let timestamp_us = meta.timestamp.sec as u64 * 1_000_000 + meta.timestamp.usec as u64;

        let (data, format) = if is_yuyv {
            (yuyv_to_bgra(buf), FrameFormat::BGRA)
        } else {
            (buf.to_vec(), FrameFormat::MJPEG)
        };

        let frame = Frame {
            data,
            width: w,
            height: h,
            format,
            timestamp_us,
        };
        if tx.send(frame).is_err() {
            break; // receiver dropped → camera closed
        }
    }
}

// ---------------------------------------------------------------------------
// Colour-space conversion: YUYV (YUV 4:2:2 packed) → BGRA
// ---------------------------------------------------------------------------

/// Converts a YUYV buffer into 32-bit BGRA pixel data.
///
/// YUYV encodes two horizontally adjacent pixels as four bytes `[Y0 U Y1 V]`.
/// Conversion uses the BT.601 "full range" formula with integer arithmetic.
fn yuyv_to_bgra(yuyv: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(yuyv.len() * 2);
    let clamp = |v: i32| v.clamp(0, 255) as u8;
    for chunk in yuyv.chunks_exact(4) {
        let y0 = chunk[0] as i32;
        let u = chunk[1] as i32;
        let y1 = chunk[2] as i32;
        let v = chunk[3] as i32;
        for &y in &[y0, y1] {
            let c = y - 16;
            let d = u - 128;
            let e = v - 128;
            let r = clamp((298 * c + 409 * e + 128) >> 8);
            let g = clamp((298 * c - 100 * d - 208 * e + 128) >> 8);
            let b = clamp((298 * c + 516 * d + 128) >> 8);
            out.extend_from_slice(&[b, g, r, 255]);
        }
    }
    out
}
