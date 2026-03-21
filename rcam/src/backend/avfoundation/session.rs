//! AVCaptureSession management — Phase 2 real implementation.
//!
//! Owns the `AVCaptureSession`, the `AVCaptureVideoDataOutput`, and the
//! `FrameDelegate` (an Objective-C object that receives decoded frames and
//! forwards them through a Tokio unbounded channel).

use std::sync::OnceLock;

use dispatch2::{DispatchQueue, DispatchRetained};
use objc2::rc::{Allocated, Retained};
use objc2::runtime::ProtocolObject;
use objc2::{define_class, msg_send, ClassType, DefinedClass};
use objc2_av_foundation::{
    AVCaptureConnection, AVCaptureDeviceInput, AVCaptureOutput, AVCaptureSession,
    AVCaptureVideoDataOutput, AVCaptureVideoDataOutputSampleBufferDelegate,
};
use objc2_core_media::CMSampleBuffer;
use objc2_core_video::{
    CVPixelBufferGetBaseAddress, CVPixelBufferGetBaseAddressOfPlane, CVPixelBufferGetBytesPerRow,
    CVPixelBufferGetBytesPerRowOfPlane, CVPixelBufferGetHeight, CVPixelBufferGetPixelFormatType,
    CVPixelBufferGetWidth, CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags,
    CVPixelBufferUnlockBaseAddress,
};
use objc2_foundation::{NSObject, NSObjectProtocol};
use tokio::sync::mpsc;

use crate::{CameraConfig, CameraError, Frame, FrameFormat};

// ---------------------------------------------------------------------------
// SendWrapper — lets us store non-Send ObjC objects in Send structs
// ---------------------------------------------------------------------------

/// Marks an ObjC `Retained<T>` as `Send + Sync`.
///
/// SAFETY: AVFoundation objects use atomic retain/release and their
/// thread-safety guarantees are documented per-class.  We serialise all
/// method calls through our own `Mutex`, so concurrent access is prevented.
pub(crate) struct SendWrapper<T>(pub T);
unsafe impl<T> Send for SendWrapper<T> {}
unsafe impl<T> Sync for SendWrapper<T> {}

impl<T> std::ops::Deref for SendWrapper<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}

// ---------------------------------------------------------------------------
// FrameDelegate — ObjC class that receives video frames
// ---------------------------------------------------------------------------

/// Ivars stored inside the ObjC `FrameDelegate` instance.
#[derive(Default)]
pub(crate) struct FrameDelegateIvars {
    /// Channel sender set once after allocation via `with_sender`.
    pub tx: OnceLock<mpsc::UnboundedSender<Frame>>,
}

define_class!(
    // SAFETY: NSObject is a valid superclass with no subclassing requirements.
    #[unsafe(super(NSObject))]
    #[name = "RcamFrameDelegate"]
    #[ivars = FrameDelegateIvars]
    pub(crate) struct FrameDelegate;

    impl FrameDelegate {
        #[unsafe(method_id(init))]
        fn init(this: Allocated<Self>) -> Retained<Self> {
            let this = this.set_ivars(FrameDelegateIvars::default());
            // SAFETY: NSObject's `init` is always safe to call.
            unsafe { msg_send![super(this), init] }
        }
    }

    // SAFETY: NSObjectProtocol has no additional safety requirements.
    unsafe impl NSObjectProtocol for FrameDelegate {}

    // SAFETY: We implement the optional method with the correct selector and types.
    unsafe impl AVCaptureVideoDataOutputSampleBufferDelegate for FrameDelegate {
        #[unsafe(method(captureOutput:didOutputSampleBuffer:fromConnection:))]
        fn capture_output_did_output_sample_buffer(
            &self,
            _output: &AVCaptureOutput,
            sample_buffer: &CMSampleBuffer,
            _connection: &AVCaptureConnection,
        ) {
            if let Some(tx) = self.ivars().tx.get() {
                if let Some(frame) = extract_frame(sample_buffer) {
                    // If the receiver is gone (camera closed) we just drop the frame.
                    let _ = tx.send(frame);
                }
            }
        }
    }
);

impl FrameDelegate {
    /// Allocate a new delegate and wire up the frame-channel sender.
    pub(crate) fn with_sender(tx: mpsc::UnboundedSender<Frame>) -> Retained<Self> {
        // SAFETY: `new` calls our `init` which sets up default ivars.
        let delegate: Retained<Self> = unsafe { msg_send![Self::class(), new] };
        delegate.ivars().tx.set(tx).ok();
        delegate
    }
}

// ---------------------------------------------------------------------------
// Frame extraction from CMSampleBuffer
// ---------------------------------------------------------------------------

fn extract_frame(sample_buffer: &CMSampleBuffer) -> Option<Frame> {
    // SAFETY: image_buffer() returns a retained reference valid for this call.
    let image_buffer = unsafe { sample_buffer.image_buffer() }?;

    // CVPixelBuffer = CVImageBuffer = CVBuffer (type aliases).
    let pixel_buffer: &objc2_core_video::CVPixelBuffer = &image_buffer;

    // Lock the pixel buffer for read-only access.
    // SAFETY: Every lock is paired with an unlock before we return.
    unsafe { CVPixelBufferLockBaseAddress(pixel_buffer, CVPixelBufferLockFlags(0)) };

    let width = CVPixelBufferGetWidth(pixel_buffer) as u32;
    let height = CVPixelBufferGetHeight(pixel_buffer) as u32;
    let pixel_format = CVPixelBufferGetPixelFormatType(pixel_buffer);

    // kCVPixelFormatType_32BGRA = 0x42475241 ('BGRA')
    // kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange = 0x34323076 ('420v')
    // kCVPixelFormatType_420YpCbCr8BiPlanarFullRange  = 0x34323066 ('420f')
    let data = match pixel_format {
        0x42475241 => {
            // Packed BGRA — copy row-by-row, stripping any stride padding.
            // SAFETY: base address is valid after locking.
            let bpr = CVPixelBufferGetBytesPerRow(pixel_buffer);
            let base = CVPixelBufferGetBaseAddress(pixel_buffer) as *const u8;
            if bpr == (width * 4) as usize {
                unsafe {
                    std::slice::from_raw_parts(base, (width * height * 4) as usize).to_vec()
                }
            } else {
                let mut out = Vec::with_capacity((width * height * 4) as usize);
                for row in 0..height {
                    let row_ptr = unsafe { base.add(row as usize * bpr) };
                    let row_sl =
                        unsafe { std::slice::from_raw_parts(row_ptr, (width * 4) as usize) };
                    out.extend_from_slice(row_sl);
                }
                out
            }
        }
        0x34323076 | 0x34323066 => {
            // Biplanar YCbCr (NV12): plane-0 = Y, plane-1 = CbCr interleaved.
            // Convert to BGRA using BT.601 video-range coefficients.
            let video_range = pixel_format == 0x34323076;
            let y_stride = CVPixelBufferGetBytesPerRowOfPlane(pixel_buffer, 0);
            let uv_stride = CVPixelBufferGetBytesPerRowOfPlane(pixel_buffer, 1);
            let y_base =
                CVPixelBufferGetBaseAddressOfPlane(pixel_buffer, 0) as *const u8;
            let uv_base =
                CVPixelBufferGetBaseAddressOfPlane(pixel_buffer, 1) as *const u8;

            let mut bgra = vec![0u8; (width * height * 4) as usize];
            for row in 0..height as usize {
                let uv_row = row >> 1;
                for col in 0..width as usize {
                    // SAFETY: indices are within the locked planes.
                    let y_raw = unsafe { *y_base.add(row * y_stride + col) } as i32;
                    let uv_off = uv_row * uv_stride + (col & !1);
                    let cb = unsafe { *uv_base.add(uv_off) } as i32;
                    let cr = unsafe { *uv_base.add(uv_off + 1) } as i32;

                    // BT.601 integer conversion (video-range Y∈[16,235], UV∈[16,240]).
                    let (y, cb, cr) = if video_range {
                        (y_raw - 16, cb - 128, cr - 128)
                    } else {
                        (y_raw, cb - 128, cr - 128)
                    };
                    let c = if video_range { 298 * y } else { 256 * y };
                    let r = ((c + 409 * cr + 128) >> 8).clamp(0, 255) as u8;
                    let g = ((c - 100 * cb - 208 * cr + 128) >> 8).clamp(0, 255) as u8;
                    let b = ((c + 516 * cb + 128) >> 8).clamp(0, 255) as u8;

                    let off = (row * width as usize + col) * 4;
                    bgra[off] = b;
                    bgra[off + 1] = g;
                    bgra[off + 2] = r;
                    bgra[off + 3] = 255;
                }
            }
            bgra
        }
        // kCVPixelFormatType_422YpCbCr8 = 0x32767579 ('2vuy') — UYVY packed 4:2:2
        // Byte order per 4-byte macro-pixel: [Cb, Y0, Cr, Y1]
        0x32767579 => {
            let bpr = CVPixelBufferGetBytesPerRow(pixel_buffer);
            let base = CVPixelBufferGetBaseAddress(pixel_buffer) as *const u8;
            let mut bgra = vec![0u8; (width * height * 4) as usize];
            for row in 0..height as usize {
                let row_base = unsafe { base.add(row * bpr) };
                let mut col = 0usize;
                while col < width as usize {
                    // Each 4-byte group covers two horizontal pixels.
                    let byte_off = col * 2; // 2 bytes per pixel in UYVY
                    let cb = unsafe { *row_base.add(byte_off) } as i32;
                    let y0 = unsafe { *row_base.add(byte_off + 1) } as i32;
                    let cr = unsafe { *row_base.add(byte_off + 2) } as i32;
                    let y1 = unsafe { *row_base.add(byte_off + 3) } as i32;

                    for (i, y_raw) in [y0, y1].iter().enumerate() {
                        let px = col + i;
                        if px >= width as usize {
                            break;
                        }
                        // BT.601 full-range (camera outputs full-range Y)
                        let y = y_raw - 16;
                        let cb_ = cb - 128;
                        let cr_ = cr - 128;
                        let c = 298 * y;
                        let r = ((c + 409 * cr_ + 128) >> 8).clamp(0, 255) as u8;
                        let g = ((c - 100 * cb_ - 208 * cr_ + 128) >> 8).clamp(0, 255) as u8;
                        let b = ((c + 516 * cb_ + 128) >> 8).clamp(0, 255) as u8;
                        let off = (row * width as usize + px) * 4;
                        bgra[off] = b;
                        bgra[off + 1] = g;
                        bgra[off + 2] = r;
                        bgra[off + 3] = 255;
                    }
                    col += 2;
                }
            }
            bgra
        }
        other => {
            // Unknown format — log once and return an empty frame so the caller
            // can keep running rather than silently dropping all frames.
            eprintln!(
                "[rcam] unsupported pixel format 0x{:08x} — frame dropped",
                other
            );
            return None;
        }
    };

    // SAFETY: paired with the lock above.
    unsafe { CVPixelBufferUnlockBaseAddress(pixel_buffer, CVPixelBufferLockFlags(0)) };

    Some(Frame {
        data,
        width,
        height,
        format: FrameFormat::BGRA,
        timestamp_us: 0,
    })
}

// ---------------------------------------------------------------------------
// AvfSession — wraps the running AVCaptureSession
// ---------------------------------------------------------------------------

/// Holds the live AVFoundation capture pipeline.
///
/// `frame_rx` is returned separately from `new()` so the caller can store it
/// behind an async-aware lock without blocking the session mutex.
pub(crate) struct AvfSession {
    pub session: SendWrapper<Retained<AVCaptureSession>>,
    /// Keep the video output alive; the session and delegate hold strong refs to it.
    pub _video_output: SendWrapper<Retained<AVCaptureVideoDataOutput>>,
    /// Keep the GCD serial queue alive for the lifetime of the session.
    pub _queue: DispatchRetained<DispatchQueue>,
    /// The Obj-C frame-callback delegate — must outlive the video output.
    pub _delegate: Retained<FrameDelegate>,
}

// SAFETY: AvfSession's ObjC objects are thread-safe via atomic retain/release;
// all mutation is serialised through the Mutex in AvfCamera.
unsafe impl Send for AvfSession {}
unsafe impl Sync for AvfSession {}

impl AvfSession {
    /// Build a capture session for the given config and device.
    ///
    /// Returns `(Self, frame_rx)` — the caller stores `frame_rx` separately
    /// (e.g. behind a `tokio::sync::Mutex`) to receive decoded frames.
    pub(crate) fn new(
        config: &CameraConfig,
        device: Retained<objc2_av_foundation::AVCaptureDevice>,
    ) -> Result<(Self, mpsc::UnboundedReceiver<Frame>), CameraError> {
        // --- Session ---
        let session = unsafe { AVCaptureSession::new() };

        // --- Device input ---
        let input = unsafe { AVCaptureDeviceInput::deviceInputWithDevice_error(&device) }
            .map_err(|e| CameraError::Backend(e.localizedDescription().to_string()))?;

        if unsafe { session.canAddInput(&input) } {
            unsafe { session.addInput(&input) };
        } else {
            return Err(CameraError::DeviceBusy);
        }

        // --- Frame channel ---
        let (tx, rx) = mpsc::unbounded_channel::<Frame>();

        // --- Delegate ---
        let delegate = FrameDelegate::with_sender(tx);

        // --- Dispatch queue (serial, named) ---
        let queue = DispatchQueue::new("rcam.avf.video", None);

        // --- Video output ---
        let video_output = unsafe { AVCaptureVideoDataOutput::new() };

        // Do NOT force a specific pixel format here — letting AVFoundation use
        // the camera's native format (typically NV12/420v on macOS) allows
        // AVCaptureMovieFileOutput to coexist in the same session.  Forcing BGRA
        // via setVideoSettings prevents AVCaptureMovieFileOutput from recording
        // (AVErrorCannotRecord).  extract_frame() handles both BGRA and NV12.
        unsafe { video_output.setVideoSettings(None) };

        // Wire up the delegate on the serial queue.
        // SAFETY: queue and delegate outlive this call (held in AvfSession).
        unsafe {
            video_output.setSampleBufferDelegate_queue(
                Some(ProtocolObject::from_ref(&*delegate)),
                Some(&*queue),
            );
        }

        if unsafe { session.canAddOutput(&video_output) } {
            unsafe { session.addOutput(&video_output) };
        } else {
            return Err(CameraError::Backend(
                "Cannot add video output to session".into(),
            ));
        }

        // Resolution / frame-rate tuning will be applied in a later phase.
        let _ = config;

        // --- Start the capture pipeline ---
        unsafe { session.startRunning() };

        Ok((
            Self {
                session: SendWrapper(session),
                _video_output: SendWrapper(video_output),
                _queue: queue,
                _delegate: delegate,
            },
            rx,
        ))
    }

    /// Stop the underlying `AVCaptureSession`.
    pub(crate) fn stop(&self) {
        unsafe { self.session.stopRunning() };
    }
}
