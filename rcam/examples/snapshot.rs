//! Example: open the default camera and save a single photo to disk.
//!
//! Run with:
//!   cargo run --example snapshot -- [output_path]
//!
//! The output path defaults to `snapshot.png`. Requires the `image-output`
//! feature to be enabled (`--features image-output`) for PNG encoding.

use std::env;
use std::path::PathBuf;

use rcam::{Camera, CameraConfig, CameraDevice};

#[tokio::main]
async fn main() {
    let output: PathBuf = env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("snapshot.bgra"));

    println!("Opening default camera…");
    let cam = Camera::open(CameraConfig::default())
        .await
        .expect("Failed to open camera");

    println!("Taking photo…");
    let frame = cam.take_photo().await.expect("Failed to take photo");

    println!(
        "Captured {}×{} frame ({:?}, {} bytes)",
        frame.width,
        frame.height,
        frame.format,
        frame.data.len()
    );

    std::fs::write(&output, &frame.data).expect("Failed to write output file");
    println!("Saved raw pixel data to {}", output.display());

    cam.close().await.expect("Failed to close camera");
}
