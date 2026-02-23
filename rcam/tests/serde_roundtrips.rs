//! Round-trip serialisation tests for all public `serde`-gated types.
//!
//! Compiled only when the crate is built with `--features serde`
//! (declared via `required-features` in Cargo.toml).

use rcam::{CameraCapabilities, CameraInfo, CameraPosition, Frame, FrameFormat, Resolution};

// ---------------------------------------------------------------------------
// Resolution
// ---------------------------------------------------------------------------

#[test]
fn resolution_roundtrip() {
    let original = Resolution {
        width: 1920,
        height: 1080,
    };
    let json = serde_json::to_string(&original).unwrap();
    let decoded: Resolution = serde_json::from_str(&json).unwrap();
    assert_eq!(original, decoded);
}

#[test]
fn resolution_serialises_as_expected_json() {
    let res = Resolution {
        width: 640,
        height: 480,
    };
    let json = serde_json::to_string(&res).unwrap();
    assert!(json.contains("640"));
    assert!(json.contains("480"));
}

// ---------------------------------------------------------------------------
// CameraPosition
// ---------------------------------------------------------------------------

#[test]
fn camera_position_all_variants_roundtrip() {
    for pos in [
        CameraPosition::Front,
        CameraPosition::Back,
        CameraPosition::External,
        CameraPosition::Unknown,
    ] {
        let json = serde_json::to_string(&pos).unwrap();
        let decoded: CameraPosition = serde_json::from_str(&json).unwrap();
        assert_eq!(pos, decoded, "failed for {pos:?}");
    }
}

// ---------------------------------------------------------------------------
// FrameFormat
// ---------------------------------------------------------------------------

#[test]
fn frame_format_all_variants_roundtrip() {
    for fmt in [
        FrameFormat::MJPEG,
        FrameFormat::NV12,
        FrameFormat::YUV420,
        FrameFormat::BGRA,
        FrameFormat::RGB24,
    ] {
        let json = serde_json::to_string(&fmt).unwrap();
        let decoded: FrameFormat = serde_json::from_str(&json).unwrap();
        assert_eq!(fmt, decoded, "failed for {fmt:?}");
    }
}

// ---------------------------------------------------------------------------
// CameraInfo
// ---------------------------------------------------------------------------

#[test]
fn camera_info_roundtrip() {
    let info = CameraInfo {
        id: "cam-0".to_string(),
        name: "Built-in Camera".to_string(),
        position: CameraPosition::Front,
        is_default: true,
    };
    let json = serde_json::to_string(&info).unwrap();
    let decoded: CameraInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.id, info.id);
    assert_eq!(decoded.name, info.name);
    assert_eq!(decoded.position, info.position);
    assert_eq!(decoded.is_default, info.is_default);
}

// ---------------------------------------------------------------------------
// Frame
// ---------------------------------------------------------------------------

#[test]
fn frame_roundtrip() {
    let frame = Frame {
        data: vec![0xDE, 0xAD, 0xBE, 0xEF],
        width: 1,
        height: 1,
        format: FrameFormat::BGRA,
        timestamp_us: 123_456_789,
    };
    let json = serde_json::to_string(&frame).unwrap();
    let decoded: Frame = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.data, frame.data);
    assert_eq!(decoded.width, frame.width);
    assert_eq!(decoded.height, frame.height);
    assert_eq!(decoded.format, frame.format);
    assert_eq!(decoded.timestamp_us, frame.timestamp_us);
}

// ---------------------------------------------------------------------------
// CameraCapabilities
// ---------------------------------------------------------------------------

#[test]
fn camera_capabilities_roundtrip() {
    let caps = CameraCapabilities {
        supported_resolutions: vec![
            Resolution {
                width: 640,
                height: 480,
            },
            Resolution {
                width: 1920,
                height: 1080,
            },
        ],
        supported_frame_rates: vec![24, 30, 60],
        supported_formats: vec![FrameFormat::NV12, FrameFormat::BGRA],
        has_torch: true,
        has_zoom: false,
    };
    let json = serde_json::to_string(&caps).unwrap();
    let decoded: CameraCapabilities = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.supported_frame_rates, caps.supported_frame_rates);
    assert_eq!(decoded.has_torch, caps.has_torch);
    assert_eq!(decoded.has_zoom, caps.has_zoom);
    assert_eq!(
        decoded.supported_resolutions.len(),
        caps.supported_resolutions.len()
    );
}
