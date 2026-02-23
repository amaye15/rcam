//! Record five seconds of video and save to `recording.mp4`.
//!
//! Run with:
//! ```
//! cargo run --example record
//! ```

use std::path::PathBuf;

use rcam::{CameraConfig, CameraDevice, RecordingOutput, VideoOutput};

const RECORD_SECS: u64 = 5;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = CameraConfig::default();
    let mut camera = rcam::Camera::open(config).await?;

    camera.start_stream().await?;

    let output_path = PathBuf::from("recording.mp4");
    println!("Recording {} seconds to {}…", RECORD_SECS, output_path.display());

    camera
        .start_recording(RecordingOutput::File(output_path.clone()))
        .await?;

    tokio::time::sleep(std::time::Duration::from_secs(RECORD_SECS)).await;

    let video = camera.stop_recording().await?;

    match video.kind {
        VideoOutput::File(path) => println!("Saved recording to {}", path.display()),
        VideoOutput::Buffer(bytes) => {
            std::fs::write(&output_path, &bytes)?;
            println!(
                "Saved in-memory recording to {} ({} bytes)",
                output_path.display(),
                bytes.len()
            );
        }
    }

    camera.stop_stream().await?;
    camera.close().await?;

    Ok(())
}
