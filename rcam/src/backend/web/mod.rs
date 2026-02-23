//! Web (WASM) backend — Phase 3 implementation.
//!
//! Uses browser APIs via `web-sys`:
//!
//! | Operation        | Browser API                       |
//! |------------------|-----------------------------------|
//! | Enumeration      | `navigator.mediaDevices.enumerateDevices` |
//! | Open / stream    | `navigator.mediaDevices.getUserMedia` |
//! | Frame capture    | `<canvas>.getImageData` (RGBA→BGRA) |
//! | Still photo      | Same as frame capture              |
//! | Video recording  | `MediaRecorder` (webm Blob buffer) |
//!
//! # WASM-specific constraints
//! - All operations are on the browser's single JS thread; no `Send` required.
//! - Recording always returns `VideoOutput::Buffer` — browsers have no
//!   direct filesystem write access in a standard context.
//! - Frame capture works by drawing the live `<video>` element to an
//!   off-screen `<canvas>` and reading back `getImageData` (RGBA).
//!   The bytes are byte-swapped R↔B to produce the crate-wide BGRA format.

pub mod capture;
pub mod recorder;

use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;

use crate::traits::CameraDevice;
use crate::{
    CameraCapabilities, CameraConfig, CameraError, CameraInfo, Frame, FrameFormat, RecordingOutput,
    Resolution, VideoData, VideoOutput,
};

use capture::js_err;
use recorder::WebRecorder;

// ---------------------------------------------------------------------------
// WebCamera
// ---------------------------------------------------------------------------

/// WASM camera handle backed by `navigator.mediaDevices.getUserMedia`.
pub struct WebCamera {
    /// Live `MediaStream` obtained from the browser.
    stream: web_sys::MediaStream,
    /// Off-screen `<video>` element playing the stream.
    video: web_sys::HtmlVideoElement,
    /// Off-screen `<canvas>` for frame extraction via `getImageData`.
    /// Kept alive here so the JS GC doesn't collect it while `ctx` is in use.
    _canvas: web_sys::HtmlCanvasElement,
    /// 2-D rendering context on the canvas.
    ctx: web_sys::CanvasRenderingContext2d,
    /// Pixel dimensions to stamp on captured `Frame`s.
    resolution: Resolution,
    /// Active `MediaRecorder` session, if recording.
    recording: Option<WebRecorder>,
    capabilities: CameraCapabilities,
}

#[async_trait::async_trait(?Send)]
impl CameraDevice for WebCamera {
    // -----------------------------------------------------------------------
    // Enumeration
    // -----------------------------------------------------------------------

    async fn enumerate() -> Result<Vec<CameraInfo>, CameraError>
    where
        Self: Sized,
    {
        capture::enumerate_devices().await
    }

    // -----------------------------------------------------------------------
    // Open
    // -----------------------------------------------------------------------

    async fn open(config: CameraConfig) -> Result<Self, CameraError>
    where
        Self: Sized,
    {
        let stream = capture::get_stream(&config).await?;

        let document = web_sys::window()
            .and_then(|w| w.document())
            .ok_or_else(|| CameraError::Backend("no DOM document available".into()))?;

        // Create an off-screen <video> element to host the stream.
        let video = document
            .create_element("video")
            .map_err(|e| CameraError::Backend(js_err(e)))?
            .dyn_into::<web_sys::HtmlVideoElement>()
            .map_err(|_| CameraError::Backend("create_element('video') failed".into()))?;

        video.set_muted(true);
        video.set_autoplay(true);
        // `playsInline` prevents fullscreen on iOS.
        js_sys::Reflect::set(&video, &"playsInline".into(), &JsValue::TRUE).ok();
        video.set_src_object(Some(&stream));

        // `play()` returns a Promise — await it so the stream is active.
        JsFuture::from(video.play().map_err(|e| CameraError::Backend(js_err(e)))?)
            .await
            .map_err(|e| CameraError::Backend(js_err(e)))?;

        // Wait for `loadeddata` so at least one video frame is available.
        wait_for_event(&video, "loadeddata").await?;

        // Determine actual stream dimensions (may differ from config).
        let w = video.video_width().max(config.resolution.width);
        let h = video.video_height().max(config.resolution.height);
        let resolution = Resolution {
            width: w,
            height: h,
        };

        // Create an off-screen <canvas> matching the stream resolution.
        let canvas = document
            .create_element("canvas")
            .map_err(|e| CameraError::Backend(js_err(e)))?
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .map_err(|_| CameraError::Backend("create_element('canvas') failed".into()))?;
        canvas.set_width(w);
        canvas.set_height(h);

        let ctx = canvas
            .get_context("2d")
            .map_err(|e| CameraError::Backend(js_err(e)))?
            .ok_or_else(|| CameraError::Backend("failed to get 2D canvas context".into()))?
            .dyn_into::<web_sys::CanvasRenderingContext2d>()
            .map_err(|_| CameraError::Backend("canvas context is not 2D".into()))?;

        Ok(Self {
            stream,
            video,
            _canvas: canvas,
            ctx,
            resolution,
            recording: None,
            capabilities: CameraCapabilities {
                supported_resolutions: vec![resolution],
                supported_frame_rates: vec![30],
                supported_formats: vec![FrameFormat::BGRA],
                has_torch: false,
                has_zoom: false,
            },
        })
    }

    // -----------------------------------------------------------------------
    // Streaming
    // -----------------------------------------------------------------------

    async fn start_stream(&mut self) -> Result<(), CameraError> {
        // The stream is already playing after `open()`; nothing extra needed.
        Ok(())
    }

    async fn capture_frame(&self) -> Result<Frame, CameraError> {
        grab_frame(&self.ctx, &self.video, self.resolution)
    }

    async fn take_photo(&self) -> Result<Frame, CameraError> {
        // The browser's auto-exposure has already settled by the time
        // `loadeddata` fired in `open()`; return the current frame directly.
        grab_frame(&self.ctx, &self.video, self.resolution)
    }

    // -----------------------------------------------------------------------
    // Recording
    // -----------------------------------------------------------------------

    async fn start_recording(&mut self, output: RecordingOutput) -> Result<(), CameraError> {
        if matches!(output, RecordingOutput::File(_)) {
            return Err(CameraError::Unsupported);
        }
        if self.recording.is_some() {
            return Err(CameraError::Backend("recording already in progress".into()));
        }
        self.recording = Some(WebRecorder::start(&self.stream)?);
        Ok(())
    }

    async fn stop_recording(&mut self) -> Result<VideoData, CameraError> {
        let rec = self.recording.take().ok_or(CameraError::NotRecording)?;
        let bytes = rec.stop().await?;
        Ok(VideoData {
            kind: VideoOutput::Buffer(bytes),
        })
    }

    // -----------------------------------------------------------------------
    // Misc
    // -----------------------------------------------------------------------

    async fn stop_stream(&mut self) -> Result<(), CameraError> {
        // Nothing to flush — frames are pulled on-demand from the canvas.
        Ok(())
    }

    fn capabilities(&self) -> &CameraCapabilities {
        &self.capabilities
    }

    async fn close(self) -> Result<(), CameraError>
    where
        Self: Sized,
    {
        // Stop any active recording.
        if let Some(rec) = self.recording {
            let _ = rec.stop().await;
        }
        // Stop all tracks in the MediaStream to release the camera hardware.
        for track in self.stream.get_video_tracks().iter() {
            track
                .dyn_into::<web_sys::MediaStreamTrack>()
                .map(|t| t.stop())
                .ok();
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Frame capture helper
// ---------------------------------------------------------------------------

/// Draw the current video frame to `ctx` and read back BGRA pixel data.
fn grab_frame(
    ctx: &web_sys::CanvasRenderingContext2d,
    video: &web_sys::HtmlVideoElement,
    resolution: Resolution,
) -> Result<Frame, CameraError> {
    let w = resolution.width as f64;
    let h = resolution.height as f64;

    ctx.draw_image_with_html_video_element(video, 0.0, 0.0)
        .map_err(|e| CameraError::Backend(js_err(e)))?;

    let image_data = ctx
        .get_image_data(0.0, 0.0, w, h)
        .map_err(|e| CameraError::Backend(js_err(e)))?;

    // `getImageData` returns RGBA. Swap R ↔ B to produce BGRA, matching the
    // other backends.
    let mut data = image_data.data().0;
    for pixel in data.chunks_exact_mut(4) {
        pixel.swap(0, 2); // R ↔ B
    }

    Ok(Frame {
        data,
        width: resolution.width,
        height: resolution.height,
        format: FrameFormat::BGRA,
        timestamp_us: timestamp_us(),
    })
}

// ---------------------------------------------------------------------------
// DOM / JS helpers
// ---------------------------------------------------------------------------

/// Wait for a named DOM event on `target` by wrapping a one-shot listener
/// in a manually-resolved JS `Promise`.
async fn wait_for_event(
    target: &web_sys::HtmlVideoElement,
    event_name: &str,
) -> Result<(), CameraError> {
    use js_sys::Promise;

    let mut resolve_fn: Option<js_sys::Function> = None;
    let promise = Promise::new(&mut |resolve: js_sys::Function, _: js_sys::Function| {
        resolve_fn = Some(resolve);
    });
    let resolve = resolve_fn.unwrap();

    let cb = Closure::once(move |_: web_sys::Event| {
        resolve.call0(&JsValue::NULL).ok();
    });

    target
        .add_event_listener_with_callback(event_name, cb.as_ref().unchecked_ref())
        .map_err(|e| CameraError::Backend(js_err(e)))?;
    cb.forget();

    JsFuture::from(promise)
        .await
        .map_err(|e| CameraError::Backend(js_err(e)))?;

    Ok(())
}

/// Current time in microseconds since the Unix epoch.
///
/// Uses `Date.now()` (millisecond precision) scaled to microseconds.
fn timestamp_us() -> u64 {
    (js_sys::Date::now() * 1_000.0) as u64
}
