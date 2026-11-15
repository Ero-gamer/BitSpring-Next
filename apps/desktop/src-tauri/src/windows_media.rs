//! Windows system integration: the System Media Transport Controls (SMTC) — the player in the
//! volume flyout and on the lock screen — and the commands that come from it, from the
//! keyboard's media keys (⏮ ⏯ ⏭) and from Bluetooth headphones.
//!
//! Track changes go to the window, like the phone remote's, so they behave exactly as its
//! buttons do (the queue there may hold streaming tracks). Same shape as `apple_media.rs`, so
//! `lib.rs` drives every platform the same way.

use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use tauri::{AppHandle, Manager};
use windows::Foundation::{TimeSpan, TypedEventHandler};
use windows::Media::{
    MediaPlaybackStatus, MediaPlaybackType, PlaybackPositionChangeRequestedEventArgs, SystemMediaTransportControls,
    SystemMediaTransportControlsButton, SystemMediaTransportControlsButtonPressedEventArgs,
    SystemMediaTransportControlsTimelineProperties,
};
use windows::Storage::Streams::{DataWriter, InMemoryRandomAccessStream, RandomAccessStreamReference};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::WinRT::ISystemMediaTransportControlsInterop;
use windows::core::{HSTRING, Ref, Result, factory};

use crate::AppState;

/// What the system player shows for the current track.
pub struct TrackInfo {
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration: f64,
    pub cover: Option<Vec<u8>>,
}

/// What we last told Windows, so play/pause/seek updates go out only when something changed.
struct Published {
    rev: u64,
    duration: f64,
    paused: bool,
    position: f64,
    at: Instant,
}

/// The window's controls (agile: usable from any thread).
static SMTC: OnceLock<SystemMediaTransportControls> = OnceLock::new();
static PUBLISHED: Mutex<Option<Published>> = Mutex::new(None);

/// Seconds as WinRT's 100-ns ticks.
fn ticks(secs: f64) -> TimeSpan {
    TimeSpan { Duration: (secs.max(0.0) * 10_000_000.0) as i64 }
}

/// Hook up the controls to the main window. Call once, on the main thread, after it exists.
pub fn install(app: &AppHandle) {
    if let Err(e) = try_install(app) {
        eprintln!("system media controls unavailable: {e}");
    }
}

fn try_install(app: &AppHandle) -> Result<()> {
    let Some(window) = app.get_webview_window("main") else { return Ok(()) };
    let Ok(hwnd) = window.hwnd() else { return Ok(()) };
    let interop = factory::<SystemMediaTransportControls, ISystemMediaTransportControlsInterop>()?;
    // SAFETY: `hwnd` is the live top-level window of this process.
    let smtc: SystemMediaTransportControls = unsafe { interop.GetForWindow(HWND(hwnd.0))? };
    smtc.SetIsEnabled(true)?;
    smtc.SetIsPlayEnabled(true)?;
    smtc.SetIsPauseEnabled(true)?;
    smtc.SetIsNextEnabled(true)?;
    smtc.SetIsPreviousEnabled(true)?;
    smtc.SetPlaybackStatus(MediaPlaybackStatus::Closed)?;

    let a = app.clone();
    smtc.ButtonPressed(&TypedEventHandler::new(
        move |_, args: Ref<SystemMediaTransportControlsButtonPressedEventArgs>| {
            match args.ok()?.Button()? {
                SystemMediaTransportControlsButton::Play => paused(&a, false),
                SystemMediaTransportControlsButton::Pause => paused(&a, true),
                SystemMediaTransportControlsButton::Next => crate::remote_command(&a, "next"),
                SystemMediaTransportControlsButton::Previous => crate::remote_command(&a, "prev"),
                _ => {}
            }
            Ok(())
        },
    ))?;
    let a = app.clone();
    smtc.PlaybackPositionChangeRequested(&TypedEventHandler::new(
        move |_, args: Ref<PlaybackPositionChangeRequestedEventArgs>| {
            let at = args.ok()?.RequestedPlaybackPosition()?;
            crate::seek_to(&a.state::<AppState>(), at.Duration as f64 / 10_000_000.0);
            Ok(())
        },
    ))?;
    let _ = SMTC.set(smtc);
    Ok(())
}

/// Play or pause what is loaded; "play" with nothing loaded starts the window's queue, as its
/// own play button does. (Windows sends Play or Pause for the one ⏯ key, by the status we set.)
fn paused(app: &AppHandle, value: bool) {
    let state = app.state::<AppState>();
    let loaded = match state.player.lock().unwrap().as_ref() {
        Some(p) => {
            p.set_paused(value);
            true
        }
        None => false,
    };
    if !loaded && !value {
        crate::remote_command(app, "toggle");
    }
}

/// A new track started.
pub fn set_track(_app: &AppHandle, rev: u64, t: TrackInfo) {
    let Some(smtc) = SMTC.get() else { return };
    let p = Published { rev, duration: t.duration, paused: false, position: 0.0, at: Instant::now() };
    let _ = publish(smtc, &p);
    *PUBLISHED.lock().unwrap() = Some(p);
    // Writing the cover into a stream waits on WinRT: not on the queue thread.
    std::thread::spawn(move || {
        if let Err(e) = show(smtc, rev, &t) {
            eprintln!("system media controls: {e}");
        }
    });
}

/// Title, artist, album and cover — unless another track took over meanwhile.
fn show(smtc: &SystemMediaTransportControls, rev: u64, t: &TrackInfo) -> Result<()> {
    let thumbnail = t.cover.as_deref().map(thumbnail).transpose()?;
    let current = || PUBLISHED.lock().unwrap().as_ref().is_some_and(|p| p.rev == rev);
    if !current() {
        return Ok(());
    }
    let u = smtc.DisplayUpdater()?;
    u.ClearAll()?;
    u.SetType(MediaPlaybackType::Music)?;
    let m = u.MusicProperties()?;
    m.SetTitle(&HSTRING::from(&t.title))?;
    if let Some(a) = &t.artist {
        m.SetArtist(&HSTRING::from(a))?;
    }
    if let Some(a) = &t.album {
        m.SetAlbumTitle(&HSTRING::from(a))?;
    }
    if let Some(th) = &thumbnail {
        u.SetThumbnail(th)?;
    }
    u.Update()
}

fn thumbnail(bytes: &[u8]) -> Result<RandomAccessStreamReference> {
    let stream = InMemoryRandomAccessStream::new()?;
    let w = DataWriter::CreateDataWriter(&stream)?;
    w.WriteBytes(bytes)?;
    w.StoreAsync()?.join()?;
    w.FlushAsync()?.join()?;
    w.DetachStream()?;
    stream.Seek(0)?;
    RandomAccessStreamReference::CreateFromStream(&stream)
}

fn publish(smtc: &SystemMediaTransportControls, p: &Published) -> Result<()> {
    smtc.SetPlaybackStatus(if p.paused { MediaPlaybackStatus::Paused } else { MediaPlaybackStatus::Playing })?;
    let tl = SystemMediaTransportControlsTimelineProperties::new()?;
    tl.SetStartTime(ticks(0.0))?;
    tl.SetEndTime(ticks(p.duration))?;
    tl.SetMinSeekTime(ticks(0.0))?;
    tl.SetMaxSeekTime(ticks(p.duration))?;
    tl.SetPosition(ticks(p.position))?;
    smtc.UpdateTimelineProperties(&tl)
}

/// Nothing is loaded any more.
pub fn clear(_app: &AppHandle) {
    *PUBLISHED.lock().unwrap() = None;
    let Some(smtc) = SMTC.get() else { return };
    let _ = smtc.SetPlaybackStatus(MediaPlaybackStatus::Stopped);
    if let Ok(u) = smtc.DisplayUpdater() {
        let _ = u.ClearAll();
        let _ = u.Update();
    }
}

/// Called from the queue thread every tick: resend status and position when the user paused,
/// resumed or seeked.
pub fn sync(_app: &AppHandle, rev: u64, position: f64, paused: bool) {
    let Some(smtc) = SMTC.get() else { return };
    let mut g = PUBLISHED.lock().unwrap();
    let Some(p) = g.as_mut().filter(|p| p.rev == rev) else { return };
    let expected = if p.paused { p.position } else { p.position + p.at.elapsed().as_secs_f64() };
    if p.paused != paused || (position - expected).abs() > 1.0 {
        p.paused = paused;
        p.position = position;
        p.at = Instant::now();
        let _ = publish(smtc, p);
    }
}
