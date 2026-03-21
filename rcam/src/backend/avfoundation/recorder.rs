//! `AVCaptureMovieFileOutput`-based video recording.
//!
//! # Threading model
//!
//! `startRecordingToOutputFileURL` and `stopRecording` must be called from the
//! same OS thread, and AVFoundation dispatches `didStartRecordingToOutputFileAtURL`
//! back to the calling thread's RunLoop.
//!
//! `didFinishRecordingToOutputFileAtURL` is dispatched on the GCD main queue,
//! which the tokio runtime does not drain.  We therefore use `outputFileURL`
//! becoming `nil` as the "recording is done" signal — this happens synchronously
//! inside `stopRecording()` and the file is complete at that point.
//!
//! The entire recording lifecycle (start → wait → stop) runs in a single
//! `spawn_blocking` closure so that AVFoundation has a stable OS thread for
//! its RunLoop-based callbacks.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use objc2::rc::{Allocated, Retained};
use objc2::runtime::ProtocolObject;
use objc2::{define_class, msg_send, ClassType};
use objc2_av_foundation::{
    AVCaptureConnection, AVCaptureFileOutput, AVCaptureFileOutputRecordingDelegate,
    AVCaptureMovieFileOutput, AVCaptureSession,
};
use objc2_foundation::{
    NSArray, NSDate, NSDefaultRunLoopMode, NSError, NSObject, NSObjectProtocol, NSRunLoop,
    NSString, NSURL,
};

use crate::CameraError;

use super::session::SendWrapper;

// ---------------------------------------------------------------------------
// RunLoop helper — confines NSDefaultRunLoopMode extern-static unsafe here
// ---------------------------------------------------------------------------

/// Spin the current thread's RunLoop for one short interval.
fn run_loop_spin(run_loop: &NSRunLoop, interval_secs: f64) {
    let wake = NSDate::dateWithTimeIntervalSinceNow(interval_secs);
    // SAFETY: NSDefaultRunLoopMode is a valid, fully-initialized ObjC constant.
    unsafe { run_loop.runMode_beforeDate(NSDefaultRunLoopMode, &wake) };
}

// ---------------------------------------------------------------------------
// RecordingDelegate — ObjC delegate (kept for protocol conformance; the
// required `didFinishRecording` is a no-op because it fires on the GCD main
// queue which tokio doesn't drain — see module doc for details)
// ---------------------------------------------------------------------------

define_class!(
    // SAFETY: NSObject is a valid superclass with no subclassing requirements.
    #[unsafe(super(NSObject))]
    #[name = "RcamRecordingDelegate"]
    #[ivars = ()]
    pub(crate) struct RecordingDelegate;

    impl RecordingDelegate {
        #[unsafe(method_id(init))]
        fn init(this: Allocated<Self>) -> Retained<Self> {
            let this = this.set_ivars(());
            unsafe { msg_send![super(this), init] }
        }
    }

    // SAFETY: NSObjectProtocol has no additional safety requirements.
    unsafe impl NSObjectProtocol for RecordingDelegate {}

    // SAFETY: We implement the required method with the correct selector and types.
    unsafe impl AVCaptureFileOutputRecordingDelegate for RecordingDelegate {
        // Required method — must be present for protocol conformance even
        // though we don't rely on it for flow control.
        #[unsafe(method(captureOutput:didFinishRecordingToOutputFileAtURL:fromConnections:error:))]
        fn capture_output_did_finish_recording(
            &self,
            _output: &AVCaptureFileOutput,
            _output_file_url: &NSURL,
            _connections: &NSArray<AVCaptureConnection>,
            _error: Option<&NSError>,
        ) {
            // Dispatched on the GCD main queue; flow control is via outputFileURL polling.
        }
    }
);

impl RecordingDelegate {
    pub(crate) fn new() -> Retained<Self> {
        // SAFETY: `new` calls our `init` which initialises the ivars.
        unsafe { msg_send![Self::class(), new] }
    }
}

// ---------------------------------------------------------------------------
// AvfRecorder — manages the recording lifecycle on a dedicated blocking thread
// ---------------------------------------------------------------------------

/// Manages a live `AVCaptureMovieFileOutput` recording.
pub(crate) struct AvfRecorder {
    /// The movie file output added to the session (for `removeOutput` later).
    pub movie_output: SendWrapper<Retained<AVCaptureMovieFileOutput>>,
    /// Send `()` here to trigger `stopRecording()` on the recording thread.
    stop_tx: std::sync::mpsc::SyncSender<()>,
    /// Await this to learn the final recording result.
    result_rx: tokio::sync::oneshot::Receiver<Result<(), CameraError>>,
    /// The output file path.
    pub output_path: PathBuf,
    /// `true` when the file is a temp file that should be read and deleted.
    pub is_temp: bool,
}

impl AvfRecorder {
    /// Attach an `AVCaptureMovieFileOutput` to `session`, then launch a
    /// `spawn_blocking` task that calls `startRecordingToOutputFileURL` and
    /// waits for a stop signal before calling `stopRecording()`.
    pub(crate) fn start(
        session: &AVCaptureSession,
        path: PathBuf,
        is_temp: bool,
    ) -> Result<Self, CameraError> {
        let movie_output = unsafe { AVCaptureMovieFileOutput::new() };

        // Wrap addOutput in begin/commitConfiguration so AVFoundation atomically
        // establishes connections on a running session before we start recording.
        // SAFETY: all three methods are valid to call on a running AVCaptureSession.
        let added = unsafe {
            session.beginConfiguration();
            let ok = session.canAddOutput(&movie_output);
            if ok {
                session.addOutput(&movie_output);
            }
            session.commitConfiguration();
            ok
        };
        if !added {
            return Err(CameraError::Backend(
                "Cannot add movie output to session".into(),
            ));
        }

        // Resolve to an absolute path — AVFoundation requires an absolute file URL.
        let path = if path.is_absolute() {
            path
        } else {
            std::env::current_dir()
                .map_err(|e| CameraError::Backend(e.to_string()))?
                .join(&path)
        };

        // AVFoundation errors if the output file already exists.
        if path.exists() {
            std::fs::remove_file(&path)
                .map_err(|e| CameraError::Backend(format!("Cannot remove existing file: {e}")))?;
        }

        let (stop_tx, stop_rx) = std::sync::mpsc::sync_channel::<()>(1);
        let (result_tx, result_rx) = tokio::sync::oneshot::channel::<Result<(), CameraError>>();

        let delegate = RecordingDelegate::new();
        let movie_sw = SendWrapper(movie_output.clone());
        let delegate_sw = SendWrapper(delegate);
        let path_clone = path.clone();

        tokio::task::spawn_blocking(move || {
            let movie = movie_sw;
            let _delegate = delegate_sw; // keep alive for AVFoundation

            let path_str = match path_clone.to_str() {
                Some(s) => s.to_owned(),
                None => {
                    let _ = result_tx.send(Err(CameraError::Backend(
                        "Recording path is not valid UTF-8".into(),
                    )));
                    return;
                }
            };

            // Build URL inside the blocking thread — NSURL is not Send.
            let ns_path = NSString::from_str(&path_str);
            let url = NSURL::fileURLWithPath(&ns_path);

            // Start recording. Callbacks from AVFoundation arrive on this
            // thread's RunLoop (which we spin below).
            unsafe {
                movie.startRecordingToOutputFileURL_recordingDelegate(
                    &url,
                    ProtocolObject::from_ref(&**_delegate),
                );
            }

            // Spin the RunLoop while waiting for the stop signal.
            // This lets AVFoundation deliver didStartRecording and catch any
            // immediate errors (e.g. AVErrorCannotRecord) via outputFileURL.
            let run_loop = NSRunLoop::currentRunLoop();
            loop {
                run_loop_spin(&run_loop, 0.05);
                match stop_rx.try_recv() {
                    Ok(()) | Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
                    Err(std::sync::mpsc::TryRecvError::Empty) => {}
                }
            }

            // stopRecording() sets outputFileURL to nil synchronously and
            // finalises the file. didFinishRecording is dispatched on the GCD
            // main queue (which tokio doesn't drain), so we poll outputFileURL
            // instead of waiting for the callback.
            unsafe { movie.stopRecording() };

            // Spin the RunLoop briefly to let AVFoundation flush any remaining
            // I/O and deliver any pending callbacks.
            let flush_deadline = Instant::now() + Duration::from_millis(500);
            while Instant::now() < flush_deadline {
                run_loop_spin(&run_loop, 0.05);
            }

            // outputFileURL is nil → recording is finalised.
            let result = if unsafe { movie.outputFileURL() }.is_none() {
                Ok(())
            } else {
                Err(CameraError::Backend(
                    "Recording did not stop within expected time".into(),
                ))
            };
            let _ = result_tx.send(result);
        });

        Ok(Self {
            movie_output: SendWrapper(movie_output),
            stop_tx,
            result_rx,
            output_path: path,
            is_temp,
        })
    }

    /// Signal the recording thread to call `stopRecording()`, then await the result.
    pub(crate) async fn stop_and_wait(self) -> Result<(), CameraError> {
        let _ = self.stop_tx.send(());
        self.result_rx
            .await
            .map_err(|_| CameraError::Backend("Recording task terminated unexpectedly".into()))?
    }
}
