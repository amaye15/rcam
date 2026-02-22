fn main() {
    // Only generate bindings when targeting Android.
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() != "android" {
        return;
    }

    let ndk = std::env::var("ANDROID_NDK_ROOT")
        .or_else(|_| std::env::var("ANDROID_NDK_HOME"))
        .expect(
            "ANDROID_NDK_ROOT (or ANDROID_NDK_HOME) must be set when building for Android. \
             Download the NDK from https://developer.android.com/ndk/downloads",
        );

    // Minimum API 24 (Android 7.0) — required for Camera2 NDK.
    let _api_level = "24";

    // Determine the host prebuilt toolchain directory.
    // On Linux CI this is linux-x86_64; on macOS it is darwin-x86_64.
    let host = if cfg!(target_os = "macos") {
        "darwin-x86_64"
    } else {
        "linux-x86_64"
    };

    let sysroot = format!("{ndk}/toolchains/llvm/prebuilt/{host}/sysroot");
    let include = format!("{sysroot}/usr/include");

    // Link the required NDK shared libraries.
    println!("cargo:rustc-link-lib=camera2ndk");
    println!("cargo:rustc-link-lib=mediandk");
    println!("cargo:rustc-link-lib=android");

    // Rerun if any of the relevant NDK headers change.
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=ANDROID_NDK_ROOT");
    println!("cargo:rerun-if-env-changed=ANDROID_NDK_HOME");

    #[cfg(feature = "bindgen")]
    {
        let bindings = bindgen::Builder::default()
            .header(format!("{include}/camera/NdkCameraManager.h"))
            .header(format!("{include}/camera/NdkCameraDevice.h"))
            .header(format!("{include}/camera/NdkCameraCaptureSession.h"))
            .header(format!("{include}/camera/NdkCameraMetadata.h"))
            .header(format!("{include}/camera/NdkCameraMetadataTags.h"))
            .header(format!("{include}/media/NdkImage.h"))
            .header(format!("{include}/media/NdkImageReader.h"))
            .header(format!("{include}/media/NdkMediaRecorder.h"))
            .header(format!("{include}/media/NdkMediaMuxer.h"))
            .clang_arg(format!("--sysroot={sysroot}"))
            .clang_arg(format!("-isystem{include}"))
            .allowlist_function("ACamera.*")
            .allowlist_function("AImage.*")
            .allowlist_function("AMediaRecorder.*")
            .allowlist_function("AMediaMuxer.*")
            .allowlist_type("ACamera.*")
            .allowlist_type("AImage.*")
            .allowlist_var("ACAMERA.*")
            .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
            .generate()
            .expect("Unable to generate Camera2 NDK bindings");

        let out_path = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
        bindings
            .write_to_file(out_path.join("camera2_bindings.rs"))
            .expect("Failed to write Camera2 NDK bindings");
    }
}
