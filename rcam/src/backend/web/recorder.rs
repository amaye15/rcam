//! Browser `MediaRecorder`-based video recording — Phase 3 implementation.
//!
//! The browser always records to in-memory Blob chunks; `VideoOutput::File`
//! is rejected with `CameraError::Unsupported` (no filesystem access in a
//! standard browser context).

use std::cell::RefCell;
use std::rc::Rc;

use js_sys::Promise;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;

use crate::CameraError;

use super::capture::js_err;

// ---------------------------------------------------------------------------
// WebRecorder
// ---------------------------------------------------------------------------

/// Active `MediaRecorder` session, collecting Blob chunks from `dataavailable`.
pub struct WebRecorder {
    recorder: web_sys::MediaRecorder,
    /// Collected chunks from `dataavailable` events, shared with the closure.
    chunks: Rc<RefCell<Vec<web_sys::Blob>>>,
}

impl WebRecorder {
    /// Start recording from `stream`.
    ///
    /// Uses `video/webm` (widest browser-native format) with a 100 ms chunk
    /// interval so there's always data available even for short recordings.
    pub fn start(stream: &web_sys::MediaStream) -> Result<Self, CameraError> {
        // Prefer webm; fall back to whatever the browser supports.
        let opts = web_sys::MediaRecorderOptions::new();
        opts.set_mime_type("video/webm");

        let recorder =
            web_sys::MediaRecorder::new_with_media_stream_and_media_recorder_options(
                stream, &opts,
            )
            .or_else(|_| web_sys::MediaRecorder::new_with_media_stream(stream))
            .map_err(|e| CameraError::Backend(js_err(e)))?;

        let chunks: Rc<RefCell<Vec<web_sys::Blob>>> = Rc::new(RefCell::new(Vec::new()));
        let chunks_clone = chunks.clone();

        // Collect Blob segments as they arrive.
        let on_data = Closure::<dyn FnMut(web_sys::BlobEvent)>::new(
            move |event: web_sys::BlobEvent| {
                if let Some(blob) = event.data() {
                    if blob.size() > 0.0 {
                        chunks_clone.borrow_mut().push(blob);
                    }
                }
            },
        );
        recorder.set_ondataavailable(Some(on_data.as_ref().unchecked_ref()));
        // `forget` keeps the closure alive for the recording session. This is
        // acceptable because recording sessions are finite and page-bounded.
        on_data.forget();

        recorder
            .start_with_time_slice(100)
            .map_err(|e| CameraError::Backend(js_err(e)))?;

        Ok(Self { recorder, chunks })
    }

    /// Stop the recorder and return all recorded bytes as a single `Vec<u8>`.
    ///
    /// Waits for the browser to flush the final chunk (the `onstop` event),
    /// then reads the combined Blob as an `ArrayBuffer`.
    pub async fn stop(self) -> Result<Vec<u8>, CameraError> {
        // Create a JS Promise whose resolve function we fire from `onstop`.
        // The Promise executor runs synchronously, so `resolve_fn` is always
        // populated by the time `Promise::new` returns.
        let mut resolve_fn: Option<js_sys::Function> = None;
        let done_promise = Promise::new(
            &mut |resolve: js_sys::Function, _reject: js_sys::Function| {
                resolve_fn = Some(resolve);
            },
        );
        let resolve = resolve_fn.unwrap();

        let onstop = Closure::once(move |_: web_sys::Event| {
            resolve.call0(&JsValue::NULL).ok();
        });
        self.recorder
            .set_onstop(Some(onstop.as_ref().unchecked_ref()));
        onstop.forget();

        self.recorder
            .stop()
            .map_err(|e| CameraError::Backend(js_err(e)))?;

        // Block until the browser fires `onstop`.
        JsFuture::from(done_promise)
            .await
            .map_err(|e| CameraError::Backend(js_err(e)))?;

        // Merge all Blob chunks into one, then read as raw bytes.
        let chunk_arr = js_sys::Array::new();
        for blob in self.chunks.borrow().iter() {
            chunk_arr.push(blob);
        }
        if chunk_arr.length() == 0 {
            return Ok(Vec::new());
        }

        let combined = web_sys::Blob::new_with_blob_sequence(&chunk_arr)
            .map_err(|e| CameraError::Backend(js_err(e)))?;

        // `Blob::array_buffer()` returns `Promise` directly (no Result).
        let ab = JsFuture::from(combined.array_buffer())
            .await
            .map_err(|e| CameraError::Backend(js_err(e)))?;

        Ok(js_sys::Uint8Array::new(&ab).to_vec())
    }
}
