//! List all cameras visible to the operating system.
//!
//! Run with:
//! ```
//! cargo run --example list_devices
//! ```

use rcam::CameraDevice;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let devices = rcam::Camera::enumerate().await?;

    if devices.is_empty() {
        println!("No cameras found.");
        return Ok(());
    }

    println!("{} camera(s) found:", devices.len());
    for cam in &devices {
        println!(
            "  [{pos:?}] {name}\n    id={id}  default={default}",
            pos = cam.position,
            name = cam.name,
            id = cam.id,
            default = cam.is_default,
        );
    }

    Ok(())
}
