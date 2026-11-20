//! What the listener marked: favourite tracks and albums, and ratings.
//!
//! Kept apart from the library index (a rescan rebuilds that from the files) and from the
//! files themselves: on this device, sent nowhere. A track is known by its path, so a file
//! that was moved or renamed starts unmarked; an album is known by its id, which its tags
//! make — and those the tag editor can change, so a favourite album also remembers one of
//! its tracks and is found again by it (see [`Marks::follow_albums`]).

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

/// The most stars a track can have.
pub const MAX_RATING: u8 = 5;

/// A track's marks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mark {
    /// A favourite.
    #[serde(default)]
    pub fav: bool,
    /// One to five stars; 0: not rated.
    #[serde(default)]
    pub rating: u8,
    /// When it became a favourite (Unix seconds).
    #[serde(default)]
    pub at: u64,
}

impl Mark {
    fn is_empty(&self) -> bool {
        !self.fav && self.rating == 0
    }
}

/// A favourite album.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlbumMark {
    /// When it became a favourite (Unix seconds).
    #[serde(default)]
    pub at: u64,
    /// One of its tracks: what finds the album again once its tags, and with them its id,
    /// have changed.
    #[serde(default)]
    pub track: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Marks {
    /// By track path. Only tracks that carry a mark are here.
    #[serde(default)]
    pub tracks: HashMap<String, Mark>,
    /// By album id.
    #[serde(default)]
    pub albums: HashMap<String, AlbumMark>,
}

impl Marks {
    /// The marks of the track at `path` (none: the default).
    pub fn of(&self, path: &str) -> Mark {
        self.tracks.get(path).copied().unwrap_or_default()
    }

    pub fn album(&self, id: &str) -> bool {
        self.albums.contains_key(id)
    }

    /// Change one track's marks; a track left with none is forgotten. True: something changed.
    fn edit(&mut self, path: &str, f: impl FnOnce(&mut Mark)) -> bool {
        let before = self.of(path);
        let mut after = before;
        f(&mut after);
        if after == before {
            return false;
        }
        if after.is_empty() {
            self.tracks.remove(path);
        } else {
            self.tracks.insert(path.to_string(), after);
        }
        true
    }

    /// Make the track a favourite (at `now`, Unix seconds) or no longer one. True: it changed.
    pub fn set_favorite(&mut self, path: &str, on: bool, now: u64) -> bool {
        self.edit(path, |m| {
            if m.fav != on {
                m.fav = on;
                m.at = if on { now } else { 0 };
            }
        })
    }

    /// Give the track `rating` stars (0 takes them away; more than five is five). True: it changed.
    pub fn set_rating(&mut self, path: &str, rating: u8) -> bool {
        self.edit(path, |m| m.rating = rating.min(MAX_RATING))
    }

    /// Make the album a favourite or no longer one. `track`: one of its tracks' paths.
    /// True: it changed.
    pub fn set_album(&mut self, id: &str, on: bool, track: &str, now: u64) -> bool {
        if on == self.album(id) {
            return false;
        }
        if on {
            self.albums.insert(id.to_string(), AlbumMark { at: now, track: track.to_string() });
        } else {
            self.albums.remove(id);
        }
        true
    }

    /// The library was scanned (`albums`: each one's id and its tracks' paths). A favourite
    /// album that is no longer there under its id, while the track it remembers now belongs to
    /// another album, was renamed: the mark moves to that album. An album whose track is
    /// nowhere (an unplugged drive) keeps its mark for when it is back. True: something moved.
    pub fn follow_albums(&mut self, albums: &[(String, Vec<String>)]) -> bool {
        let present: HashSet<&str> = albums.iter().map(|(id, _)| id.as_str()).collect();
        let mut owner: HashMap<&str, &str> = HashMap::new();
        for (id, paths) in albums {
            for p in paths {
                owner.insert(p, id);
            }
        }
        let mut changed = false;
        // The track an album remembers must be one it still has: files get removed too.
        for (id, paths) in albums {
            if let Some(mark) = self.albums.get_mut(id)
                && owner.get(mark.track.as_str()) != Some(&id.as_str())
                && let Some(first) = paths.first()
            {
                mark.track = first.clone();
                changed = true;
            }
        }
        let moved: Vec<(String, String)> = self
            .albums
            .iter()
            .filter(|(id, _)| !present.contains(id.as_str()))
            .filter_map(|(id, mark)| Some((id.clone(), owner.get(mark.track.as_str())?.to_string())))
            .collect();
        for (old, new) in moved {
            let Some(mark) = self.albums.remove(&old) else { continue };
            // (Renamed into an album that is a favourite already: that one's mark stays.)
            self.albums.entry(new).or_insert(mark);
            changed = true;
        }
        changed
    }

    /// Every path as `f` gives it now: for a platform that moves the app's folder under the
    /// saved paths (an iPhone's container). True: something moved.
    pub fn rebase(&mut self, f: impl Fn(&str) -> String) -> bool {
        let mut changed = false;
        let tracks = std::mem::take(&mut self.tracks);
        for (path, mark) in tracks {
            let now = f(&path);
            changed |= now != path;
            self.tracks.insert(now, mark);
        }
        for mark in self.albums.values_mut() {
            let now = f(&mark.track);
            changed |= now != mark.track;
            mark.track = now;
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_favourite_remembers_when_and_is_forgotten_when_unmarked() {
        let mut m = Marks::default();
        assert!(m.set_favorite("/a.flac", true, 100));
        assert_eq!(m.of("/a.flac"), Mark { fav: true, rating: 0, at: 100 });
        // Marked again later: nothing changes, the first time stands.
        assert!(!m.set_favorite("/a.flac", true, 200));
        assert_eq!(m.of("/a.flac").at, 100);
        assert!(m.set_favorite("/a.flac", false, 300));
        assert!(m.tracks.is_empty());
        assert!(!m.set_favorite("/a.flac", false, 400));
    }

    #[test]
    fn a_rating_stands_beside_the_heart() {
        let mut m = Marks::default();
        assert!(m.set_rating("/a.flac", 4));
        assert!(m.set_favorite("/a.flac", true, 100));
        assert!(m.set_favorite("/a.flac", false, 200));
        // The stars keep the track on record after the heart is gone.
        assert_eq!(m.of("/a.flac"), Mark { fav: false, rating: 4, at: 0 });
        assert!(m.set_rating("/a.flac", 9));
        assert_eq!(m.of("/a.flac").rating, MAX_RATING);
        assert!(m.set_rating("/a.flac", 0));
        assert!(m.tracks.is_empty());
    }

    fn albums(list: &[(&str, &[&str])]) -> Vec<(String, Vec<String>)> {
        list.iter().map(|(id, paths)| (id.to_string(), paths.iter().map(|p| p.to_string()).collect())).collect()
    }

    #[test]
    fn a_renamed_album_keeps_its_heart() {
        let mut m = Marks::default();
        assert!(m.set_album("old", true, "/x/1.flac", 100));
        assert!(!m.set_album("old", true, "/x/1.flac", 200));
        // Nothing changed in the library: nothing moves.
        assert!(!m.follow_albums(&albums(&[("old", &["/x/1.flac", "/x/2.flac"]), ("other", &["/y/1.flac"])])));
        // The tag editor gave the album another title: its tracks are under a new id.
        assert!(m.follow_albums(&albums(&[("new", &["/x/1.flac", "/x/2.flac"]), ("other", &["/y/1.flac"])])));
        assert!(m.album("new") && !m.album("old"));
        assert_eq!(m.albums["new"].at, 100);
    }

    #[test]
    fn an_album_that_is_away_keeps_its_heart() {
        let mut m = Marks::default();
        m.set_album("gone", true, "/drive/1.flac", 100);
        // The drive is unplugged: the album isn't in the library, and neither is its track.
        assert!(!m.follow_albums(&albums(&[("other", &["/y/1.flac"])])));
        assert!(m.album("gone"));
    }

    #[test]
    fn an_album_remembers_a_track_it_still_has() {
        let mut m = Marks::default();
        m.set_album("a", true, "/x/1.flac", 100);
        // Its first track was deleted; later the album is renamed.
        assert!(m.follow_albums(&albums(&[("a", &["/x/2.flac", "/x/3.flac"])])));
        assert_eq!(m.albums["a"].track, "/x/2.flac");
        assert!(m.follow_albums(&albums(&[("b", &["/x/2.flac", "/x/3.flac"])])));
        assert!(m.album("b"));
    }

    #[test]
    fn two_favourites_renamed_into_one_album_leave_one_heart() {
        let mut m = Marks::default();
        m.set_album("disc1", true, "/x/1.flac", 100);
        m.set_album("whole", true, "/x/9.flac", 50);
        assert!(m.follow_albums(&albums(&[("whole", &["/x/1.flac", "/x/9.flac"])])));
        assert_eq!(m.albums.len(), 1);
        assert_eq!(m.albums["whole"].at, 50);
    }

    #[test]
    fn paths_move_with_the_apps_folder() {
        let mut m = Marks::default();
        m.set_favorite("/old/Documents/a.flac", true, 1);
        m.set_album("a", true, "/old/Documents/a.flac", 1);
        assert!(m.rebase(|p| p.replace("/old/", "/new/")));
        assert!(m.of("/new/Documents/a.flac").fav);
        assert_eq!(m.albums["a"].track, "/new/Documents/a.flac");
        assert!(!m.rebase(|p| p.to_string()));
    }

    #[test]
    fn a_file_from_an_older_version_reads() {
        let m: Marks = serde_json::from_str(r#"{"tracks":{"/a.flac":{"fav":true}}}"#).unwrap();
        assert_eq!(m.of("/a.flac"), Mark { fav: true, rating: 0, at: 0 });
        assert!(m.albums.is_empty());
    }
}
