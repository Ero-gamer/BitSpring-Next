fn main() {
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    // Lock-screen / Now Playing player and remote commands (src/apple_media.rs).
    if os == "ios" || os == "macos" {
        println!("cargo:rustc-link-lib=framework=MediaPlayer");
    }
    // `system_media`: the OS has a system player (lock screen, Control Center, media keys)
    // that `media::` drives — every platform we ship on.
    println!("cargo::rustc-check-cfg=cfg(system_media)");
    if matches!(os.as_str(), "ios" | "android" | "macos" | "windows") {
        println!("cargo:rustc-cfg=system_media");
    }
    tauri_build::build()
}
