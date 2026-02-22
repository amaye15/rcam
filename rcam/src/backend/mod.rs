// Platform backend modules — compiled only for the matching target.

#[cfg(any(target_os = "ios", target_os = "macos"))]
pub mod avfoundation;

#[cfg(target_os = "android")]
pub mod android;

#[cfg(target_os = "linux")]
pub mod v4l2;

#[cfg(target_os = "windows")]
pub mod mediafoundation;

#[cfg(target_arch = "wasm32")]
pub mod web;
