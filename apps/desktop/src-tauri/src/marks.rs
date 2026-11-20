//! The listener's marks: favourite tracks and albums, and ratings, kept in marks.json next
//! to the library index — on this device only, sent nowhere.
//!
//! The file is its own, keyed by the track's path and the album's id (see
//! hifi_engine::marks), so a rescan of the library — which rebuilds the index from the
//! files — leaves it alone. Streaming tracks have their service's own favourites and get no
//! marks here.

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use hifi_engine::UserError;
use hifi_engine::marks::{self, Mark};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::library::Library;

#[derive(Default)]
pub struct Marks {
    inner: Mutex<marks::Marks>,
    file: Option<PathBuf>,
    /// Bumped with every change, so the UI knows whether what it shows is the latest.
    rev: AtomicU64,
}

/// Everything marked, as the UI takes it.
#[derive(Serialize)]
pub struct Snapshot {
    rev: u64,
    #[serde(flatten)]
    marks: marks::Marks,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

impl Marks {
    pub fn load(app: &AppHandle) -> Self {
        let m = Self::at(app.path().app_data_dir().ok().map(|d| d.join("marks.json")));
        // An iPhone moves the app's folder on updates and restores: the marks go with the music.
        #[cfg(target_os = "ios")]
        if let Ok(documents) = app.path().document_dir() {
            let _ = m.change(|all| all.rebase(|p| crate::folders::rebase(p, &documents)));
        }
        m
    }

    /// The marks kept in `file` (None: in memory only).
    pub fn at(file: Option<PathBuf>) -> Self {
        let inner = file.as_deref().and_then(hifi_engine::store::load_json).unwrap_or_default();
        Self { inner: Mutex::new(inner), file, rev: AtomicU64::new(0) }
    }

    /// Change the marks and write them out at once: they are the listener's own words, not
    /// something to measure again. A change that can't be written isn't made, so the UI
    /// never shows a heart that is gone after a restart. Ok(false): `f` changed nothing.
    fn change(&self, f: impl FnOnce(&mut marks::Marks) -> bool) -> Result<bool, UserError> {
        let mut g = self.inner.lock().unwrap();
        let mut next = g.clone();
        if !f(&mut next) {
            return Ok(false);
        }
        if let Some(file) = &self.file {
            hifi_engine::store::save_json(file, &next).map_err(|e| UserError::WriteFailed { detail: e.to_string() })?;
        }
        *g = next;
        self.rev.fetch_add(1, Ordering::AcqRel);
        Ok(true)
    }

    pub fn rev(&self) -> u64 {
        self.rev.load(Ordering::Acquire)
    }

    /// The marks of the track at `path`.
    pub fn of(&self, path: &str) -> Mark {
        self.inner.lock().unwrap().of(path)
    }

    /// Everything marked, as it is now.
    pub fn all(&self) -> marks::Marks {
        self.inner.lock().unwrap().clone()
    }

    /// Make a track a favourite or not (None: the other of what it is). What it is afterwards.
    pub fn set_favorite(&self, path: &str, on: Option<bool>) -> Result<bool, UserError> {
        let on = on.unwrap_or_else(|| !self.of(path).fav);
        self.change(|all| all.set_favorite(path, on, now()))?;
        Ok(on)
    }

    pub fn set_rating(&self, path: &str, rating: u8) -> Result<(), UserError> {
        self.change(|all| all.set_rating(path, rating)).map(drop)
    }

    /// Make an album a favourite or not. `track`: one of its tracks' paths.
    pub fn set_album(&self, id: &str, on: bool, track: &str) -> Result<(), UserError> {
        self.change(|all| all.set_album(id, on, track, now())).map(drop)
    }

    /// The library was scanned: favourite albums whose tags changed keep their hearts.
    /// True: one moved.
    pub fn follow(&self, albums: &[(String, Vec<String>)]) -> bool {
        // (Not written: the marks stay under the album's old id, and move again next time.)
        self.change(|all| all.follow_albums(albums)).unwrap_or(false)
    }
}

/// Tell the window the marks changed without it (a scan, the phone remote, the API).
pub fn announce(app: &AppHandle) {
    if let Some(marks) = app.try_state::<Marks>() {
        let _ = app.emit("marks", marks.rev());
    }
}

/// A scan has finished: see [`Marks::follow`].
pub fn after_scan(app: &AppHandle, lib: &Library) {
    let Some(marks) = app.try_state::<Marks>() else { return };
    if marks.follow(&lib.albums_with_paths()) {
        announce(app);
    }
}

/// A local file, not a streaming service's track (those are that service's to mark).
fn markable(path: &str) -> bool {
    !path.is_empty() && !crate::stream::is_uri(path)
}

/// From the phone remote or the API: a library track's heart (None: the other of what it
/// is). What it is afterwards; None: not a library track, or it couldn't be saved.
pub fn remote_favorite(app: &AppHandle, path: &str, on: Option<bool>) -> Option<bool> {
    if !app.state::<Library>().known_paths().contains(path) {
        return None;
    }
    let on = app.state::<Marks>().set_favorite(path, on).ok()?;
    announce(app);
    Some(on)
}

/// From the API: a library album's heart (None: the other of what it is). What it is
/// afterwards; None: no such album, or it couldn't be saved.
pub fn remote_album(app: &AppHandle, id: &str, on: Option<bool>) -> Option<bool> {
    let track = app.state::<Library>().albums_with_paths().into_iter().find(|(a, _)| a == id)?.1.into_iter().next()?;
    let marks = app.state::<Marks>();
    let on = on.unwrap_or_else(|| !marks.all().album(id));
    marks.set_album(id, on, &track).ok()?;
    announce(app);
    Some(on)
}

/// From the API: a library track's stars. None: not a library track, or it couldn't be saved.
pub fn remote_rating(app: &AppHandle, path: &str, rating: u8) -> Option<()> {
    if !app.state::<Library>().known_paths().contains(path) {
        return None;
    }
    app.state::<Marks>().set_rating(path, rating).ok()?;
    announce(app);
    Some(())
}

#[tauri::command]
pub fn marks(marks: State<'_, Marks>) -> Snapshot {
    // (The number first: a change in between is then fetched again, not missed.)
    let rev = marks.rev();
    Snapshot { rev, marks: marks.all() }
}

/// Each answers with the marks' new revision.
#[tauri::command]
pub fn mark_favorite(marks: State<'_, Marks>, path: String, on: bool) -> Result<u64, UserError> {
    if markable(&path) {
        marks.set_favorite(&path, Some(on))?;
    }
    Ok(marks.rev())
}

#[tauri::command]
pub fn mark_rating(marks: State<'_, Marks>, path: String, rating: u8) -> Result<u64, UserError> {
    if markable(&path) {
        marks.set_rating(&path, rating)?;
    }
    Ok(marks.rev())
}

#[tauri::command]
pub fn mark_album(lib: State<'_, Library>, marks: State<'_, Marks>, id: String, on: bool) -> Result<u64, UserError> {
    let track = lib.albums_with_paths().into_iter().find(|(a, _)| *a == id).and_then(|(_, paths)| paths.into_iter().next());
    match track {
        Some(track) => marks.set_album(&id, on, &track)?,
        // Not in the library (any more): there is nothing to make a favourite, but a heart
        // left on it can still be taken off.
        None if !on => marks.set_album(&id, false, "")?,
        None => {}
    }
    Ok(marks.rev())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bitspring-marks-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("marks.json")
    }

    #[test]
    fn marks_outlive_a_restart() {
        let file = scratch("restart");
        let m = Marks::at(Some(file.clone()));
        assert!(m.set_favorite("/a.flac", None).unwrap());
        m.set_rating("/b.flac", 4).unwrap();
        m.set_album("album", true, "/a.flac").unwrap();
        assert_eq!(m.rev(), 3);
        let again = Marks::at(Some(file));
        assert!(again.of("/a.flac").fav && again.of("/a.flac").at > 0);
        assert_eq!(again.of("/b.flac").rating, 4);
        assert!(again.all().album("album"));
        // The heart pressed again takes it off.
        assert!(!again.set_favorite("/a.flac", None).unwrap());
        assert!(!again.of("/a.flac").fav);
    }

    #[test]
    fn a_change_that_cant_be_written_isnt_made() {
        // The "file" is inside something that is a file itself: nothing can be written there.
        let blocker = scratch("unwritable");
        std::fs::write(&blocker, b"x").unwrap();
        let m = Marks::at(Some(blocker.join("marks.json")));
        assert!(m.set_favorite("/a.flac", Some(true)).is_err());
        assert!(!m.of("/a.flac").fav);
        assert_eq!(m.rev(), 0);
    }

    #[test]
    fn nothing_changed_is_no_new_revision() {
        let m = Marks::at(None);
        m.set_favorite("/a.flac", Some(false)).unwrap();
        m.set_rating("/a.flac", 0).unwrap();
        assert_eq!(m.rev(), 0);
        m.set_favorite("/a.flac", Some(true)).unwrap();
        m.set_favorite("/a.flac", Some(true)).unwrap();
        assert_eq!(m.rev(), 1);
    }

    #[test]
    fn a_renamed_album_is_followed() {
        let m = Marks::at(None);
        m.set_album("old", true, "/x/1.flac").unwrap();
        assert!(m.follow(&[("new".into(), vec!["/x/1.flac".into()])]));
        assert!(m.all().album("new"));
        assert!(!m.follow(&[("new".into(), vec!["/x/1.flac".into()])]));
    }
}
