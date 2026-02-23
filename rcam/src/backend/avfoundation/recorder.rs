//! `AVCaptureMovieFileOutput`-based video recording — Phase 2 real implementation.

use std::path::PathBuf;
use std::sync::Mutex;

use objc2::{define_class, msg_send, ClassType, DefinedClass};
use objc2::rc::{Allocated, Retained};
use objc2::runtime::ProtocolObject;
use objc2_av_foundation::{
    AVCaptureConnection, AVCaptureFileOutput, AVCaptureFileOutputRecordingDelegate,
    AVCaptureMovieFileOutput, AVCaptureSession,
};
use objc2_foundation::{NSArray, NSError, NSObject, NSObjectProtocol, NSString, NSURL};
use tokio::sync::oneshot;

use crate::CameraError;

use super::session::SendWrapper;

// ---------------------------------------------------------------------------
// RecordingDelegate — ObjC class that fires when recording completes
// ---------------------------------------------------------------------------

pub(crate) struct RecordingDelegateIvars {
    /// Fired when `didFinishRecording` arrives.  Wrapped in `Mutex` so we can
    /// move the sender out (`oneshot::Sender::send` consumes `self`).
    pub done_tx: Mutex<Option<oneshot::Sender<Result<(), String>>>>,
}

define_class!(
    // SAFETY: NSObject is a valid superclass with no subclassing requirements.
    #[unsafe(super(NSObject))]
    #[name = "RcamRecordingDelegate"]
    #[ivars = RecordingDelegateIvars]
    pub(crate) struct RecordingDelegate;

    impl RecordingDelegate {
        #[unsafe(method_id(init))]
        fn init(this: Allocated<Self>) -> Retained<Self> {
            let this = this.set_ivars(RecordingDelegateIvars {
                done_tx: Mutex::new(None),
            });
            unsafe { msg_send![super(this), init] }
        }
    }

    // SAFETY: NSObjectProtocol has no additional safety requirements.
    unsafe impl NSObjectProtocol for RecordingDelegate {}

    // SAFETY: We implement the required method with the correct selector and types.
    unsafe impl AVCaptureFileOutputRecordingDelegate for RecordingDelegate {
        #[unsafe(method(captureOutput:didFinishRecordingToOutputFileAtURL:fromConnections:error:))]
        fn capture_output_did_finish_recording(
            &self,
            _output: &AVCaptureFileOutput,
            _output_file_url: &NSURL,
            _connections: &NSArray<AVCaptureConnection>,
            error: Option<&NSError>,
        ) {
            let result = match error {
                Some(e) => Err(e.localizedDescription().to_string()),
                None => Ok(()),
            };
            if let Ok(mut guard) = self.ivars().done_tx.lock() {
                if let Some(tx) = guard.take() {
                    let _ = tx.send(result);
                }
            }
        }
    }
);

impl RecordingDelegate {
    pub(crate) fn new(tx: oneshot::Sender<Result<(), String>>) -> Retained<Self> {
        // SAFETY: `new` calls our `init` which sets the default ivar.
        let delegate: Retained<Self> = unsafe { msg_send![Self::class(), new] };
        if let Ok(mut guard) = delegate.ivars().done_tx.lock() {
            *guard = Some(tx);
        }
        delegate
    }
}

// ---------------------------------------------------------------------------
// AvfRecorder — wraps AVCaptureMovieFileOutput
// ---------------------------------------------------------------------------

/// Manages a live `AVCaptureMovieFileOutput` recording.
pub(crate) struct AvfRecorder {
    /// The movie file output added to the session.
    pub movie_output: SendWrapper<Retained<AVCaptureMovieFileOutput>>,
    /// The ObjC delegate that fires when writing completes.
    pub delegate: Retained<RecordingDelegate>,
    /// Receives the completion signal from the delegate.
    pub done_rx: oneshot::Receiver<Result<(), String>>,
    /// The output path (returned inside `VideoOutput::File`).
    pub output_path: PathBuf,
    /// Whether `output_path` is a temporary file (Buffer output mode).
    pub is_temp: bool,
}

impl AvfRecorder {
    /// Attach a `AVCaptureMovieFileOutput` to `session` and start recording
    /// to `path`.
    pub(crate) fn start(
        session: &AVCaptureSession,
        path: PathBuf,
        is_temp: bool,
    ) -> Result<Self, CameraError> {
        let movie_output = unsafe { AVCaptureMovieFileOutput::new() };

        if unsafe { session.canAddOutput(&movie_output) } {
            unsafe { session.addOutput(&movie_output) };
        } else {
            return Err(CameraError::Backend(
                "Cannot add movie output to session".into(),
            ));
        }

        let (tx, rx) = oneshot::channel();
        let delegate = RecordingDelegate::new(tx);

        let path_str = path
            .to_str()
            .ok_or_else(|| CameraError::Backend("Invalid output path".into()))?;
        let ns_path = NSString::from_str(path_str);
        let url = NSURL::fileURLWithPath(&ns_path);

        // SAFETY: `delegate` and `url` are valid for the duration of this call.
        unsafe {
            movie_output.startRecordingToOutputFileURL_recordingDelegate(
                &url,
                ProtocolObject::from_ref(&*delegate),
            );
        }

        Ok(Self {
            movie_output: SendWrapper(movie_output),
            delegate,
            done_rx: rx,
            output_path: path,
            is_temp,
        })
    }

    /// Signal AVFoundation to stop writing.  The delegate will fire when the
    /// last bytes have been flushed; await `done_rx` to get that signal.
    pub(crate) fn stop(&self) {
        unsafe { self.movie_output.stopRecording() };
    }

}
