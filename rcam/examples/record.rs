//! Example: open the default camera, record 5 seconds of video, and save it.
//!
//! Run with:
//!   cargo run --example record -- [output_path]
//!
//! The output path defaults to `recording.mp4`.

use std::env;
use std::path::PathBuf;
use std::time::Duration;

use rcam::{Camera, CameraConfig, CameraDevice, RecordingOutput, VideoOutput};

#[tokio::main]
async fn main() {
    let output: PathBuf = env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("recording.mp4"));

    println!("Opening default camera…");
    let mut cam = Camera::open(CameraConfig::default())
        .await
        .expect("Failed to open camera");

    println!("Starting recording to {}…", output.display());
    cam.start_recording(RecordingOutput::File(output.clone()))
        .await
        .expect("Failed to start recording");

    println!("Recording for 5 seconds…");
    tokio::time::sleep(Duration::from_secs(5)).await;

    println!("Stopping recording…");
    let video = cam.stop_recording().await.expect("Failed to stop recording");

    match video.kind {
        VideoOutput::File(path) => println!("Saved video to {}", path.display()),
        VideoOutput::Buffer(buf) => {
            println!("Received {} bytes in memory", buf.len());
            std::fs::write(&output, &buf).expect("Failed to write video buffer");
            println!("Saved to {}", output.display());
        }
    }

    cam.close().await.expect("Failed to close camera");
}
