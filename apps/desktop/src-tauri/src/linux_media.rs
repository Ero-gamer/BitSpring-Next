//! Linux system integration: MPRIS, the D-Bus interface every Linux desktop reads players
//! from — KDE's and GNOME's media controls, media keys,
//! Bluetooth headphones' buttons, `playerctl`.
//!
//! Same shape as `windows_media.rs`, so `lib.rs` drives every platform the same way: track
//! changes go to the window like the phone remote's commands.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use tauri::{AppHandle, Manager};
use zbus::blocking::Connection;
use zbus::blocking::object_server::InterfaceRef;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{ObjectPath, OwnedValue, Value};

use crate::AppState;

const PATH: &str = "/org/mpris/MediaPlayer2";
/// Our bus name; `.instance<pid>` would be added if a second copy ran, as the spec asks.
const NAME: &str = "org.mpris.MediaPlayer2.bitspring";

/// What the system player shows for the current track.
pub struct TrackInfo {
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration: f64,
    pub cover: Option<Vec<u8>>,
}

/// The session bus connection that serves our objects (dropping it would take them away).
static BUS: OnceLock<Connection> = OnceLock::new();

/// What we last told the desktop, so status/position updates go out only when something changed.
struct Published {
    rev: u64,
    paused: bool,
    position: f64,
    at: Instant,
}

static PUBLISHED: Mutex<Option<Published>> = Mutex::new(None);

fn micros(secs: f64) -> i64 {
    (secs.max(0.0) * 1_000_000.0) as i64
}

fn value<'a>(v: impl Into<Value<'a>>) -> OwnedValue {
    OwnedValue::try_from(v.into()).expect("plain values always convert")
}

/// org.mpris.MediaPlayer2: who we are.
struct Root {
    app: AppHandle,
}

#[zbus::interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    fn raise(&self) {
        if let Some(w) = self.app.get_webview_window("main") {
            let _ = w.unminimize();
            let _ = w.show();
            let _ = w.set_focus();
        }
    }

    fn quit(&self) {}

    #[zbus(property)]
    fn can_quit(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn can_raise(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn has_track_list(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn identity(&self) -> String {
        "BitSpring".into()
    }

    /// The .desktop file's name without the extension (the icon and name panels show).
    #[zbus(property)]
    fn desktop_entry(&self) -> String {
        "BitSpring".into()
    }

    #[zbus(property)]
    fn supported_uri_schemes(&self) -> Vec<String> {
        Vec::new()
    }

    #[zbus(property)]
    fn supported_mime_types(&self) -> Vec<String> {
        Vec::new()
    }
}

/// org.mpris.MediaPlayer2.Player: the track and the transport.
struct Player {
    app: AppHandle,
    /// Nothing loaded: status Stopped and no metadata.
    loaded: bool,
    rev: u64,
    title: String,
    artist: Option<String>,
    album: Option<String>,
    duration: f64,
    art_url: Option<String>,
    paused: bool,
}

impl Player {
    fn position_now(&self) -> f64 {
        let state = self.app.state::<AppState>();
        let player = state.player.lock().unwrap();
        player.as_ref().map(|p| p.position_secs()).unwrap_or(0.0)
    }
}

/// Play or pause what is loaded; "play" with nothing loaded starts the window's queue, as its
/// own play button does.
fn set_paused(app: &AppHandle, value: bool) {
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

#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl Player {
    fn next(&self) {
        crate::remote_command(&self.app, "next");
    }

    fn previous(&self) {
        crate::remote_command(&self.app, "prev");
    }

    fn pause(&self) {
        set_paused(&self.app, true);
    }

    fn play(&self) {
        set_paused(&self.app, false);
    }

    fn play_pause(&self) {
        if self.loaded {
            set_paused(&self.app, !self.paused);
        } else {
            crate::remote_command(&self.app, "toggle");
        }
    }

    fn stop(&self) {
        set_paused(&self.app, true);
    }

    /// Relative seek, in microseconds.
    fn seek(&self, offset: i64) {
        if !self.loaded {
            return;
        }
        let to = (self.position_now() + offset as f64 / 1_000_000.0).clamp(0.0, self.duration);
        crate::seek_to(&self.app.state::<AppState>(), to);
    }

    /// Absolute seek, for the track `track_id` (ignored if another one plays by now).
    fn set_position(&self, track_id: ObjectPath<'_>, position: i64) {
        if self.loaded && track_id.as_str() == track_path(self.rev) && position >= 0 {
            crate::seek_to(&self.app.state::<AppState>(), (position as f64 / 1_000_000.0).min(self.duration));
        }
    }

    fn open_uri(&self, _uri: String) {}

    #[zbus(signal)]
    async fn seeked(emitter: &SignalEmitter<'_>, position: i64) -> zbus::Result<()>;

    #[zbus(property)]
    fn playback_status(&self) -> String {
        match (self.loaded, self.paused) {
            (false, _) => "Stopped",
            (true, true) => "Paused",
            (true, false) => "Playing",
        }
        .into()
    }

    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        let mut m = HashMap::new();
        if !self.loaded {
            m.insert("mpris:trackid".into(), value(ObjectPath::from_static_str_unchecked("/org/mpris/MediaPlayer2/TrackList/NoTrack")));
            return m;
        }
        if let Ok(p) = ObjectPath::try_from(track_path(self.rev)) {
            m.insert("mpris:trackid".into(), value(p));
        }
        m.insert("mpris:length".into(), value(micros(self.duration)));
        m.insert("xesam:title".into(), value(self.title.as_str()));
        if let Some(a) = &self.artist {
            m.insert("xesam:artist".into(), value(vec![a.as_str()]));
        }
        if let Some(a) = &self.album {
            m.insert("xesam:album".into(), value(a.as_str()));
        }
        if let Some(u) = &self.art_url {
            m.insert("mpris:artUrl".into(), value(u.as_str()));
        }
        m
    }

    #[zbus(property)]
    fn position(&self) -> i64 {
        micros(self.position_now())
    }

    #[zbus(property)]
    fn rate(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    fn set_rate(&mut self, _rate: f64) {}

    #[zbus(property)]
    fn minimum_rate(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    fn maximum_rate(&self) -> f64 {
        1.0
    }

    /// Bit-perfect playback leaves the level to the DAC: we don't scale samples for the
    /// desktop's volume slider.
    #[zbus(property)]
    fn volume(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    fn set_volume(&mut self, _volume: f64) {}

    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_play(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_pause(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_seek(&self) -> bool {
        self.loaded
    }

    #[zbus(property)]
    fn can_control(&self) -> bool {
        true
    }
}

fn track_path(rev: u64) -> String {
    format!("/app/bitspring/track/{rev}")
}

fn player_ref() -> Option<InterfaceRef<Player>> {
    BUS.get()?.object_server().interface::<_, Player>(PATH).ok()
}

/// Put our player on the session bus. Call once, after the main window exists.
pub fn install(app: &AppHandle) {
    let built = (|| -> zbus::Result<Connection> {
        let player = Player {
            app: app.clone(),
            loaded: false,
            rev: 0,
            title: String::new(),
            artist: None,
            album: None,
            duration: 0.0,
            art_url: None,
            paused: true,
        };
        let conn = zbus::blocking::connection::Builder::session()?
            .serve_at(PATH, Root { app: app.clone() })?
            .serve_at(PATH, player)?
            .build()?;
        // A second copy running: take a name of its own, as the spec says.
        if conn.request_name(NAME).is_err() {
            conn.request_name(format!("{NAME}.instance{}", std::process::id()))?;
        }
        Ok(conn)
    })();
    match built {
        Ok(conn) => {
            let _ = BUS.set(conn);
        }
        Err(e) => eprintln!("MPRIS unavailable: {e}"),
    }
}

/// The cover as a file the desktop can show (MPRIS passes art by URL).
fn cover_file(app: &AppHandle, rev: u64, bytes: &[u8]) -> Option<String> {
    let dir = app.path().app_cache_dir().ok()?.join("mpris");
    std::fs::create_dir_all(&dir).ok()?;
    // One file per track: a desktop keeps showing a URL's old picture if the file changes.
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            let _ = std::fs::remove_file(e.path());
        }
    }
    let path = dir.join(format!("cover-{rev}"));
    std::fs::write(&path, bytes).ok()?;
    let p = path.to_str()?;
    // file:// URL: escape what can't stand in one as-is.
    let mut url = String::from("file://");
    for b in p.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' => url.push(b as char),
            _ => url.push_str(&format!("%{b:02X}")),
        }
    }
    Some(url)
}

/// A new track started.
pub fn set_track(app: &AppHandle, rev: u64, t: TrackInfo) {
    *PUBLISHED.lock().unwrap() = Some(Published { rev, paused: false, position: 0.0, at: Instant::now() });
    let app = app.clone();
    // Writing the cover and talking to the bus: not on the caller's thread.
    std::thread::spawn(move || {
        let Some(iface) = player_ref() else { return };
        let art_url = t.cover.as_deref().and_then(|b| cover_file(&app, rev, b));
        {
            let mut p = iface.get_mut();
            p.loaded = true;
            p.rev = rev;
            p.title = t.title;
            p.artist = t.artist;
            p.album = t.album;
            p.duration = t.duration;
            p.art_url = art_url;
            p.paused = false;
        }
        let p = iface.get();
        let e = iface.signal_emitter();
        let _ = zbus::block_on(async {
            p.metadata_changed(e).await?;
            p.playback_status_changed(e).await?;
            p.can_seek_changed(e).await
        });
    });
}

/// Nothing is loaded any more.
pub fn clear(_app: &AppHandle) {
    *PUBLISHED.lock().unwrap() = None;
    std::thread::spawn(|| {
        let Some(iface) = player_ref() else { return };
        {
            let mut p = iface.get_mut();
            p.loaded = false;
            p.paused = true;
            p.art_url = None;
        }
        let p = iface.get();
        let e = iface.signal_emitter();
        let _ = zbus::block_on(async {
            p.metadata_changed(e).await?;
            p.playback_status_changed(e).await?;
            p.can_seek_changed(e).await
        });
    });
}

/// Called from the queue thread every tick: tell the desktop when the user paused, resumed or
/// seeked (it extrapolates the position itself while playing).
pub fn sync(_app: &AppHandle, rev: u64, position: f64, paused: bool) {
    let mut g = PUBLISHED.lock().unwrap();
    let Some(p) = g.as_mut().filter(|p| p.rev == rev) else { return };
    let expected = if p.paused { p.position } else { p.position + p.at.elapsed().as_secs_f64() };
    let status = p.paused != paused;
    let jumped = (position - expected).abs() > 1.0;
    if !status && !jumped {
        return;
    }
    p.paused = paused;
    p.position = position;
    p.at = Instant::now();
    drop(g);
    std::thread::spawn(move || {
        let Some(iface) = player_ref() else { return };
        if status {
            iface.get_mut().paused = paused;
            let p = iface.get();
            let _ = zbus::block_on(p.playback_status_changed(iface.signal_emitter()));
        }
        if jumped {
            let _ = zbus::block_on(Player::seeked(iface.signal_emitter(), micros(position)));
        }
    });
}
