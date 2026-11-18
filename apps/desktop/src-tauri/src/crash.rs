//! Error reports that stay on the device.
//!
//! When something in the app breaks on a bug — a Rust panic on any thread; on Android also an
//! uncaught Java exception, written by App.kt — what happened is appended to crash.txt in the
//! app's data folder: the app version, the system, the error and where in our code it was.
//! The app sends it nowhere, and has no way to. On the next launch it says there is a report
//! and shows the text; whether to copy it into an issue is the user's call.
//!
//! The text carries no paths, file names or tags: source locations are cut down to the crate
//! and file, and anything else that looks like a path or an address is taken out, both when
//! the report is written and again when it is shown.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Manager};

static FILE: OnceLock<PathBuf> = OnceLock::new();
/// "macOS 15.1, aarch64" — worked out once at launch, off the main thread.
static SYSTEM: Mutex<String> = Mutex::new(String::new());

/// Past this the file starts over: a crash loop mustn't fill the disk.
const MAX_BYTES: u64 = 64 * 1024;
const SEPARATOR: &str = "----";

/// Start keeping reports. Call first thing at launch.
pub fn install(app: &AppHandle) {
    let Ok(dir) = app.path().app_data_dir() else { return };
    let _ = std::fs::create_dir_all(&dir);
    if FILE.set(dir.join("crash.txt")).is_err() {
        return;
    }
    let version = app.package_info().version.to_string();
    std::thread::spawn(move || {
        let s = system(&dir);
        *SYSTEM.lock().unwrap_or_else(|p| p.into_inner()) = s;
    });
    let before = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        record(&version, info);
        before(info);
    }));
}

/// Runs inside a panic: must not panic itself, so nothing here unwraps.
fn record(version: &str, info: &std::panic::PanicHookInfo<'_>) {
    let Some(file) = FILE.get() else { return };
    let payload = info.payload();
    let message = payload
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "an error without a message".into());
    let thread = std::thread::current();
    let location = info.location().map(|l| format!("{}:{}", short_source(l.file()), l.line()));
    let system = SYSTEM.try_lock().map(|s| s.clone()).unwrap_or_default();
    let trace = our_frames(&std::backtrace::Backtrace::force_capture().to_string());
    let text = compose(version, &system, thread.name().unwrap_or("unnamed"), &message, location.as_deref(), &trace, SystemTime::now());
    append(file, &text);
}

fn append(file: &Path, text: &str) {
    use std::io::Write;
    if std::fs::metadata(file).is_ok_and(|m| m.len() > MAX_BYTES) {
        let _ = std::fs::remove_file(file);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(file) {
        let _ = writeln!(f, "{text}\n{SEPARATOR}");
    }
}

/// One report, as the user reads it.
fn compose(version: &str, system: &str, thread: &str, message: &str, location: Option<&str>, trace: &[String], when: SystemTime) -> String {
    let mut out = format!("BitSpring {version} — error report\nTime: {} UTC\n", utc(when));
    if !system.is_empty() {
        out += &format!("System: {system}\n");
    }
    out += &format!("Thread: {thread}\nError: {}\n", scrub(message));
    if let Some(l) = location {
        out += &format!("Where: {l}\n");
    }
    if !trace.is_empty() {
        out += "Trace:\n";
        for f in trace {
            out += &format!("  {f}\n");
        }
    }
    out.trim_end().to_string()
}

/// "2026-11-18 12:40" from a clock time (days-to-date after Howard Hinnant's algorithm).
fn utc(when: SystemTime) -> String {
    let secs = when.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    let (days, rest) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let (d, m) = (doy - (153 * mp + 2) / 5 + 1, if mp < 10 { mp + 3 } else { mp - 9 });
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}", rest / 3_600, rest % 3_600 / 60)
}

/// A source file as it helps find the bug and no more: the crate and the path inside it
/// ("engine/src/output.rs"), not where the build machine keeps its checkouts.
fn short_source(file: &str) -> String {
    let parts: Vec<&str> = file.split(['/', '\\']).filter(|p| !p.is_empty()).collect();
    let from = match parts.iter().rposition(|p| *p == "src") {
        Some(i) if i > 0 => i - 1,
        _ => parts.len().saturating_sub(2),
    };
    parts[from..].join("/")
}

/// The frames of a backtrace that are in our own code, innermost first. (A release build has
/// no symbols: then there are none, and the location alone has to do.)
fn our_frames(backtrace: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in backtrace.lines() {
        let line = line.trim();
        let Some((index, symbol)) = line.split_once(": ") else { continue };
        if !index.chars().all(|c| c.is_ascii_digit()) || !(symbol.contains("hifi_") || symbol.contains("bitspring")) {
            continue;
        }
        // Without the compiler's hash on the end ("::h1a2b3c4d5e6f7a8b").
        let symbol = match symbol.rsplit_once("::h") {
            Some((name, hash)) if hash.len() == 16 && hash.chars().all(|c| c.is_ascii_hexdigit()) => name,
            _ => symbol,
        };
        if symbol.contains("crash::record") || out.last().is_some_and(|l| l == symbol) {
            continue;
        }
        out.push(symbol.to_string());
        if out.len() == 24 {
            break;
        }
    }
    out
}

/// Take out whatever looks like a path or an address: from where it starts to the end of the
/// line (a path may hold spaces, so there is no telling where it stops) — or to the closing
/// quote, when it is quoted. A file name with nothing in front of it can't be told from any
/// other word; our own messages don't print those.
fn scrub(text: &str) -> String {
    text.lines().map(scrub_line).collect::<Vec<_>>().join("\n")
}

fn scrub_line(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let boundary = |i: usize| i == 0 || !(chars[i - 1].is_alphanumeric() || matches!(chars[i - 1], '_' | '.' | '-' | '/' | '\\' | '<'));
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let rest = &chars[i..];
        let starts = boundary(i)
            && (
                // /Users/…, /storage/…
                (rest[0] == '/' && rest.get(1).is_some_and(|c| c.is_alphanumeric() || matches!(c, '.' | '_' | '~')) && rest[1..].contains(&'/'))
                // ~/Music/…
                || (rest[0] == '~' && rest.get(1) == Some(&'/'))
                // C:\Users\…, D:/Music/…
                || (rest[0].is_ascii_alphabetic() && rest.get(1) == Some(&':') && rest.get(2).is_some_and(|c| matches!(c, '\\' | '/')) && !rest.get(3).is_some_and(|c| *c == '/'))
                // \\server\share
                || (rest[0] == '\\' && rest.get(1) == Some(&'\\'))
                // file://…, content://…, https://…
                || scheme_len(rest).is_some()
            );
        if !starts {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        out.push_str("<path>");
        // Quoted: up to the closing quote. Else the rest of the line.
        let quote = (i > 0).then(|| chars[i - 1]).filter(|c| matches!(c, '"' | '\'' | '`' | '“' | '«'));
        let close = match quote {
            Some('“') => Some('”'),
            Some('«') => Some('»'),
            q => q,
        };
        match close.and_then(|q| rest.iter().position(|c| *c == q)) {
            Some(n) => i += n,
            None => break,
        }
    }
    out
}

/// Length of a URI scheme starting here ("content" in "content://media/…"), if one does.
fn scheme_len(rest: &[char]) -> Option<usize> {
    let n = rest.iter().take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')).count();
    (n >= 2 && rest[0].is_ascii_alphabetic() && rest.get(n) == Some(&':') && rest.get(n + 1) == Some(&'/') && rest.get(n + 2) == Some(&'/')).then_some(n)
}

/// The operating system and its version, as far as it can be had without asking for anything.
fn system(dir: &Path) -> String {
    let os = os_name(dir).unwrap_or_else(|| std::env::consts::OS.to_string());
    format!("{os}, {}", std::env::consts::ARCH)
}

/// Android: App.kt leaves the version and the device model in system.txt.
#[cfg(target_os = "android")]
fn os_name(dir: &Path) -> Option<String> {
    std::fs::read_to_string(dir.join("system.txt")).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn os_name(_dir: &Path) -> Option<String> {
    let name = if cfg!(target_os = "ios") { "iOS" } else { "macOS" };
    // <key>ProductVersion</key> <string>15.1</string>
    let plist = std::fs::read_to_string("/System/Library/CoreServices/SystemVersion.plist").ok()?;
    let after = plist.split("<key>ProductVersion</key>").nth(1)?;
    let version = after.split("<string>").nth(1)?.split("</string>").next()?.trim();
    Some(format!("{name} {version}"))
}

#[cfg(target_os = "linux")]
fn os_name(_dir: &Path) -> Option<String> {
    let release = std::fs::read_to_string("/etc/os-release").ok()?;
    let pretty = release.lines().find_map(|l| l.strip_prefix("PRETTY_NAME="))?;
    Some(pretty.trim_matches('"').to_string())
}

#[cfg(windows)]
fn os_name(_dir: &Path) -> Option<String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    // "Microsoft Windows [Version 10.0.22631.4460]"
    let out = std::process::Command::new("cmd").args(["/c", "ver"]).creation_flags(CREATE_NO_WINDOW).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let version = text.split("[").nth(1)?.split(']').next()?.trim();
    Some(format!("Windows ({})", version.rsplit(' ').next().unwrap_or(version)))
}

/// The reports waiting to be read (None: there are none).
#[tauri::command]
pub fn crash_report() -> Option<String> {
    let raw = std::fs::read_to_string(FILE.get()?).ok()?;
    // Scrubbed again here: the Android side writes its part itself.
    let text = scrub(&raw);
    let text = text.trim().trim_end_matches(SEPARATOR).trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// The user has dealt with the reports (sent them or not): forget them.
#[tauri::command]
pub fn crash_dismiss() {
    if let Some(f) = FILE.get() {
        let _ = std::fs::remove_file(f);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_and_addresses_never_get_into_a_report() {
        // What the app's own errors and the system's look like, with a private path in each.
        for (dirty, clean) in [
            ("can't open /Users/anna/Music/Björk/Homogenic/01 Hunter.flac: No such file", "can't open <path>"),
            ("called `Result::unwrap()` on an `Err` value: Os { code: 2, path: \"/Volumes/SSD/music/My Album/02 - Song.ape\" } at once", "called `Result::unwrap()` on an `Err` value: Os { code: 2, path: \"<path>\" } at once"),
            ("read C:\\Users\\Anna\\Music\\a b.flac failed", "read <path>"),
            ("D:/Music/x.flac", "<path>"),
            ("open ~/Music/secret tape.wav", "open <path>"),
            ("java.io.FileNotFoundException: /storage/emulated/0/Music/Anna's mix/1.mp3 (Permission denied)", "java.io.FileNotFoundException: <path>"),
            ("GET https://streaming.example/track?token=abc123 → 403", "GET <path>"),
            ("content://media/external/audio/media/42 gone", "<path>"),
            ("\\\\nas\\music\\x.flac", "<path>"),
        ] {
            assert_eq!(scrub(dirty), clean);
        }
        // What must stay readable.
        for plain in [
            "index out of bounds: the len is 3 but the index is 7",
            "assertion `left == right` failed\n  left: 44100\n right: 48000",
            "rate 44.1/48 kHz and/or 16/24 bit",
            "at dev.local.hifi.MainActivity.onCreate(MainActivity.kt:25)",
            "ratio 3:2, time 12:40",
            "Where: engine/src/output.rs:912",
        ] {
            assert_eq!(scrub(plain), plain);
        }
        // Idempotent: scrubbing what is clean changes nothing.
        assert_eq!(scrub("can't open <path>"), "can't open <path>");
    }

    #[test]
    fn source_locations_lose_the_build_machine() {
        assert_eq!(short_source("/Users/dev/projects/bitspring/crates/engine/src/output.rs"), "engine/src/output.rs");
        assert_eq!(short_source("crates/engine/src/dsp/eq.rs"), "engine/src/dsp/eq.rs");
        assert_eq!(short_source("apps/desktop/src-tauri/src/lib.rs"), "src-tauri/src/lib.rs");
        assert_eq!(short_source("/home/ci/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/symphonia-core-0.6.1/src/io/mod.rs"), "symphonia-core-0.6.1/src/io/mod.rs");
        assert_eq!(short_source("C:\\Users\\ci\\bitspring\\crates\\engine\\src\\wasapi_out.rs"), "engine/src/wasapi_out.rs");
        assert_eq!(short_source("/rustc/abc123/library/core/src/option.rs"), "core/src/option.rs");
    }

    #[test]
    fn a_report_reads_like_this() {
        let when = UNIX_EPOCH + std::time::Duration::from_secs(1_795_005_600);
        let trace = our_frames(
            "   0: std::backtrace::Backtrace::force_capture\n   1: hifi_desktop_lib::crash::record::h0123456789abcdef\n   2: core::panicking::panic_fmt\n   3: hifi_engine::output::render_loop::hfedcba9876543210\n             at ./crates/engine/src/output.rs:912:9\n   4: hifi_engine::output::render_loop::hfedcba9876543210\n   5: std::thread::Builder::spawn_unchecked_\n",
        );
        assert_eq!(trace, ["hifi_engine::output::render_loop"]);
        let text = compose("0.19.0", "macOS 15.1, aarch64", "hifi-render", "can't read /Users/anna/Music/a.flac", Some("engine/src/output.rs:912"), &trace, when);
        assert_eq!(
            text,
            "BitSpring 0.19.0 — error report\nTime: 2026-11-18 12:40 UTC\nSystem: macOS 15.1, aarch64\nThread: hifi-render\nError: can't read <path>\nWhere: engine/src/output.rs:912\nTrace:\n  hifi_engine::output::render_loop"
        );
        assert!(!text.contains("anna"));
    }

    #[test]
    fn dates_come_out_right() {
        let at = |secs: u64| utc(UNIX_EPOCH + std::time::Duration::from_secs(secs));
        assert_eq!(at(0), "1970-01-01 00:00");
        assert_eq!(at(951_782_399), "2000-02-28 23:59");
        assert_eq!(at(951_782_400), "2000-02-29 00:00");
        assert_eq!(at(1_709_251_199), "2024-02-29 23:59");
        assert_eq!(at(1_735_689_600), "2025-01-01 00:00");
    }

    /// A real panic on another thread, through the real hook, lands in the file — and the app
    /// (this test) goes on.
    #[test]
    fn a_panic_is_written_down_and_can_be_dismissed() {
        let dir = std::env::temp_dir().join(format!("bitspring-crash-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("crash.txt");
        assert!(FILE.set(file.clone()).is_ok(), "only this test sets the file");
        assert_eq!(crash_report(), None);
        let before = std::panic::take_hook();
        std::panic::set_hook(Box::new(|info| record("9.9.9", info)));
        let private = "/Users/anna/Music/Private Tape.flac";
        let joined = std::thread::Builder::new().name("hifi-test".into()).spawn(move || panic!("can't open {private}")).unwrap().join();
        std::panic::set_hook(before);
        assert!(joined.is_err());
        let report = crash_report().expect("a report");
        assert!(report.starts_with("BitSpring 9.9.9 — error report\nTime: 20"), "{report}");
        assert!(report.contains("Thread: hifi-test\nError: can't open <path>\nWhere: src-tauri/src/crash.rs:"), "{report}");
        assert!(!report.contains("anna") && !report.contains("Private Tape") && !report.ends_with(SEPARATOR), "{report}");
        // What the Android side appends is scrubbed when shown, too.
        append(&file, "Error (Android):\njava.io.FileNotFoundException: /storage/emulated/0/Music/Mix/1.mp3");
        let both = crash_report().unwrap();
        assert!(both.contains("FileNotFoundException: <path>") && !both.contains("emulated"), "{both}");
        crash_dismiss();
        assert_eq!(crash_report(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
