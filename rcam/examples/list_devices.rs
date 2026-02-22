//! Example: enumerate all cameras connected to the system.
//!
//! Run with:
//!   cargo run --example list_devices

use rcam::{Camera, CameraDevice};

#[tokio::main]
async fn main() {
    let devices = Camera::enumerate()
        .await
        .expect("Failed to enumerate cameras");

    if devices.is_empty() {
        println!("No cameras found.");
        return;
    }

    println!("Found {} camera(s):", devices.len());
    for dev in &devices {
        println!(
            "  [{id}] {name}  position={pos:?}  default={default}",
            id = dev.id,
            name = dev.name,
            pos = dev.position,
            default = dev.is_default,
        );
    }
}
