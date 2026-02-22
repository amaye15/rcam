//! V4L2 frame capture (Phase 1 stub).
//!
//! Phase 2 implementation will:
//!   1. Call `v4l::query_devices()` to list `/dev/video*` entries.
//!   2. Open the chosen device and negotiate the requested `FrameFormat`.
//!   3. Create a `v4l::io::mmap::Stream` and loop calling `stream.next()`.
//!   4. Copy the buffer into a `Frame`.
//!   5. For recording, pipe frames into `ffmpeg-next` (optional dep).

use crate::{CameraError, CameraInfo};

pub fn enumerate_devices() -> Result<Vec<CameraInfo>, CameraError> {
    // Phase 2: walk /dev/video* via v4l::query_devices().
    Ok(vec![])
}
