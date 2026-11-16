// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    #[cfg(target_os = "linux")]
    webkit_workarounds();
    hifi_desktop_lib::run()
}

/// WebKitGTK settings that keep the window from staying blank, unless the user set them:
/// - its DMA-BUF renderer leaves the page blank on some GPU drivers;
/// - with no GPU at all (a virtual machine, a remote session) its GPU compositing never shows
///   the window, so draw in software there.
#[cfg(target_os = "linux")]
fn webkit_workarounds() {
    let has_gpu = std::fs::read_dir("/dev/dri")
        .map(|d| d.flatten().any(|e| e.file_name().to_string_lossy().starts_with("renderD")))
        .unwrap_or(false);
    let set = |key: &str| {
        if std::env::var_os(key).is_none() {
            // SAFETY: first thing in main, before any other thread exists.
            unsafe { std::env::set_var(key, "1") };
        }
    };
    set("WEBKIT_DISABLE_DMABUF_RENDERER");
    if !has_gpu {
        set("WEBKIT_DISABLE_COMPOSITING_MODE");
    }
}
