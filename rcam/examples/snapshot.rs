//! Capture a single still image and save it to `snapshot.png`.
//!
//! Run with:
//! ```
//! cargo run --example snapshot --features image-output
//! ```
//!
//! The `image-output` feature is required for saving the frame as PNG.

use std::path::PathBuf;

use rcam::{CameraConfig, CameraDevice};

#[cfg(target_arch = "wasm32")]
fn main() {}

#[cfg(not(target_arch = "wasm32"))]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Open the default camera at 720p.
    let config = CameraConfig::default();
    let mut camera = rcam::Camera::open(config).await?;

    camera.start_stream().await?;

    println!("Capturing frame…");
    let frame = camera.take_photo().await?;
    println!(
        "Got frame: {}×{} {:?} ({} bytes)",
        frame.width,
        frame.height,
        frame.format,
        frame.data.len()
    );

    #[cfg(feature = "image-output")]
    {
        let img = frame.to_image()?;
        let out = PathBuf::from("snapshot.png");
        img.save(&out)?;
        println!("Saved {}", out.display());
    }

    #[cfg(not(feature = "image-output"))]
    {
        // Without image-output, write raw bytes to disk.
        let out = PathBuf::from("snapshot.raw");
        std::fs::write(&out, &frame.data)?;
        println!(
            "Saved raw frame to {} ({} bytes)",
            out.display(),
            frame.data.len()
        );
    }

    camera.stop_stream().await?;
    camera.close().await?;

    Ok(())
}
