//! Linux V4L2 device smoke tests.
//!
//! These tests require a real (or virtual) `/dev/video0` device and are
//! therefore **ignored by default**.  In CI they are enabled after loading
//! the `v4l2loopback` kernel module.
//!
//! Run manually:
//! ```
//! # Load virtual camera:
//! sudo modprobe v4l2loopback devices=1 video_nr=0 exclusive_caps=1
//!
//! # Run the ignored tests:
//! cargo test --test v4l2_device -- --include-ignored
//! ```
//!
//! In CI (Linux job only):
//! ```
//! cargo test -p rcam --test v4l2_device -- --include-ignored
//! ```

#![cfg(target_os = "linux")]

use std::path::Path;

/// Returns true if at least one `/dev/video*` device node exists.
fn any_video_device() -> bool {
    (0..=9).any(|n| Path::new(&format!("/dev/video{n}")).exists())
}

#[test]
#[ignore = "requires a V4L2 device node (real camera or v4l2loopback)"]
fn v4l2_device_node_is_accessible() {
    assert!(
        any_video_device(),
        "no /dev/video* device found; load v4l2loopback or connect a camera"
    );
}

#[test]
#[ignore = "requires a V4L2 device node (real camera or v4l2loopback)"]
fn v4l2_camera_enumerate_returns_at_least_one_device() {
    if !any_video_device() {
        eprintln!("SKIP: no V4L2 device available");
        return;
    }

    // enumerate() is async; spin up a minimal tokio runtime for the test.
    let rt = tokio::runtime::Runtime::new().unwrap();
    let devices = rt
        .block_on(rcam::Camera::enumerate())
        .expect("enumerate() must not error when a device is present");

    assert!(
        !devices.is_empty(),
        "expected at least one camera from enumerate()"
    );
    println!("Found {} V4L2 camera(s):", devices.len());
    for d in &devices {
        println!("  {} — {:?}", d.name, d.position);
    }
}

#[test]
#[ignore = "requires a V4L2 device node (real camera or v4l2loopback)"]
fn v4l2_camera_open_and_close() {
    if !any_video_device() {
        eprintln!("SKIP: no V4L2 device available");
        return;
    }

    use rcam::{CameraConfig, CameraDevice};
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let cam = rcam::Camera::open(CameraConfig::default())
            .await
            .expect("open() must succeed with a V4L2 device present");
        cam.close().await.expect("close() must not fail");
    });
}
