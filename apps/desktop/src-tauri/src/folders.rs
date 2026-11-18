//! Library folder rules that need no platform, so they can be tested anywhere: which folders
//! hold the library's tracks (the "skip a folder" list), what path an Android folder-picker
//! answer stands for, and where an iPhone's skipped folders are after its container moved.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use hifi_engine::library::{TrackInfo, file_of};
use serde::Serialize;

/// A folder holding library tracks, as the "skip a folder" list shows it.
#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Subfolder {
    pub path: String,
    pub name: String,
    /// Tracks in it, at any depth.
    pub tracks: usize,
    /// Whether any of them sit deeper than the folder itself.
    pub has_subfolders: bool,
}

/// The folder a track's file is in.
fn folder_of(t: &TrackInfo) -> Option<&Path> {
    Path::new(file_of(&t.path)).parent()
}

/// The folders right inside `parent` that hold library tracks (at any depth), by name.
/// Read off the track list: nothing touches the disk, and a skipped folder (no tracks left)
/// is not listed.
pub fn subfolders(tracks: &[TrackInfo], parent: &Path) -> Vec<Subfolder> {
    let mut found: BTreeMap<(String, String), Subfolder> = BTreeMap::new();
    for t in tracks {
        let Some(rest) = folder_of(t).and_then(|d| d.strip_prefix(parent).ok()) else { continue };
        let mut below = rest.components();
        // None: the track sits in `parent` itself.
        let Some(first) = below.next() else { continue };
        let name = first.as_os_str().to_string_lossy().into_owned();
        let entry = found.entry((name.to_lowercase(), name.clone())).or_insert_with(|| Subfolder {
            path: parent.join(first).to_string_lossy().into_owned(),
            name,
            tracks: 0,
            has_subfolders: false,
        });
        entry.tracks += 1;
        entry.has_subfolders |= below.next().is_some();
    }
    found.into_values().collect()
}

/// The library's own folders as the top of that list, in the order they were added.
pub fn roots(tracks: &[TrackInfo], folders: &[String]) -> Vec<Subfolder> {
    folders
        .iter()
        .map(|f| {
            let root = Path::new(f);
            let inside: Vec<&Path> = tracks.iter().filter_map(folder_of).filter(|d| d.starts_with(root)).collect();
            Subfolder {
                path: f.clone(),
                name: root.file_name().map_or_else(|| f.clone(), |n| n.to_string_lossy().into_owned()),
                tracks: inside.len(),
                has_subfolders: inside.iter().any(|d| *d != root),
            }
        })
        .collect()
}

/// Skipped folders no library folder holds any more (their folder was removed): forgotten,
/// so the list doesn't keep rules that do nothing.
pub fn drop_orphans(skipped: &mut Vec<String>, folders: &[String]) {
    skipped.retain(|s| folders.iter().any(|f| Path::new(s).starts_with(f)));
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// The folder an Android folder-picker answer stands for, as a path the scanner can read.
///
/// The picker answers with a tree URI, `content://<provider>/tree/<id>`. Only the system's
/// own storage provider names real folders: its ids are `<volume>:<path in it>`, the volume
/// being `primary` (internal storage, `primary_root`) or the id a memory card or USB drive
/// is mounted under in `/storage`. Anything else (a cloud drive, another app's provider) has
/// no path: None.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub fn tree_uri_path(uri: &str, primary_root: &Path) -> Option<PathBuf> {
    let rest = uri.strip_prefix("content://com.android.externalstorage.documents/tree/")?;
    // The id is one URI segment: its own slashes arrive as %2F.
    let id = percent_decode(rest.split(['/', '?', '#']).next()?)?;
    let (volume, inside) = id.split_once(':')?;
    let inside = inside.trim_matches('/');
    if inside.split('/').any(|part| part == ".." || part == ".") {
        return None;
    }
    let root = match volume {
        "primary" => primary_root.to_path_buf(),
        // The picker's "Documents" shortcut on older Androids.
        "home" => primary_root.join("Documents"),
        v if !v.is_empty() && v.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-') => Path::new("/storage").join(v),
        _ => return None,
    };
    Some(if inside.is_empty() { root } else { root.join(inside) })
}

/// A folder under an iPhone app's Documents, as it is called now: iOS moves the app's
/// container (`…/Application/<id>/Documents`) on updates and restores, so a saved path
/// names a place that may be gone.
#[cfg_attr(not(target_os = "ios"), allow(dead_code))]
pub fn rebase(path: &str, documents: &Path) -> String {
    let p = Path::new(path);
    if p.starts_with(documents) {
        return path.to_string();
    }
    let parts: Vec<_> = p.components().collect();
    let at = parts.windows(3).position(|w| w[0].as_os_str() == "Application" && w[2].as_os_str() == "Documents");
    match at {
        Some(i) => parts[i + 3..].iter().fold(documents.to_path_buf(), |d, c| d.join(c)).to_string_lossy().into_owned(),
        None => path.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(path: &str) -> TrackInfo {
        TrackInfo {
            path: path.into(),
            title: path.into(),
            artist: None,
            album_artist: None,
            album: None,
            genre: None,
            year: None,
            track_number: None,
            disc_number: None,
            duration: 1.0,
            sample_rate: 44_100,
            bits: Some(16),
            channels: 2,
            format: "FLAC".into(),
            lossless: true,
            has_cover: false,
            size: 0,
            mtime: 0,
        }
    }

    fn brief(list: &[Subfolder]) -> Vec<(&str, usize, bool)> {
        list.iter().map(|s| (s.name.as_str(), s.tracks, s.has_subfolders)).collect()
    }

    #[test]
    fn lists_the_folders_that_hold_tracks_one_level_at_a_time() {
        let tracks = [
            t("/m/loose.flac"),
            t("/m/Albums/Kind of Blue/01.flac"),
            t("/m/Albums/Kind of Blue/02.flac"),
            t("/m/Albums/single.flac"),
            t("/m/ringtones/a.mp3"),
            t("/m/Recordings/Calls/2024/x.m4a"),
            t("/music2/elsewhere.flac"),
            t("/m/Rips/Image.wav#cue:0-300"),
            t("/m/Rips/Disc.iso#sacd:3"),
        ];
        // Folder names sort without regard to case; counts include what is deeper.
        assert_eq!(
            brief(&subfolders(&tracks, Path::new("/m"))),
            [("Albums", 3, true), ("Recordings", 1, true), ("ringtones", 1, false), ("Rips", 2, false)]
        );
        assert_eq!(brief(&subfolders(&tracks, Path::new("/m/Albums"))), [("Kind of Blue", 2, false)]);
        assert_eq!(Path::new(&subfolders(&tracks, Path::new("/m/Albums"))[0].path), Path::new("/m/Albums").join("Kind of Blue"));
        assert!(subfolders(&tracks, Path::new("/m/ringtones")).is_empty());
        // A folder that only starts with the same letters is not inside.
        assert!(subfolders(&tracks, Path::new("/m/Rec")).is_empty());
        // The top of the list: the library's folders themselves.
        let top = roots(&tracks, &["/m".into(), "/music2".into(), "/empty".into()]);
        assert_eq!(brief(&top), [("m", 8, true), ("music2", 1, false), ("empty", 0, false)]);
    }

    #[test]
    fn skips_of_a_removed_folder_are_forgotten() {
        let mut skipped = vec!["/m/Podcasts".to_string(), "/card/Albums/Live".into(), "/m".into(), "/music-old/x".into()];
        drop_orphans(&mut skipped, &["/m".into()]);
        assert_eq!(skipped, ["/m/Podcasts", "/m"]);
    }

    // Android and iPhone paths: spelled with forward slashes, so checked where paths are.
    #[cfg(unix)]
    #[test]
    fn a_picked_android_folder_becomes_a_path() {
        let primary = Path::new("/storage/emulated/0");
        let path = |uri: &str| tree_uri_path(uri, primary).map(|p| p.to_string_lossy().into_owned());
        let ext = "content://com.android.externalstorage.documents/tree/";
        // Internal storage, nested, with spaces and non-Latin names.
        assert_eq!(path(&format!("{ext}primary%3AMusic")).as_deref(), Some("/storage/emulated/0/Music"));
        assert_eq!(path(&format!("{ext}primary%3AMy%20Music%2FHi-Res")).as_deref(), Some("/storage/emulated/0/My Music/Hi-Res"));
        assert_eq!(path(&format!("{ext}primary%3A%D0%9C%D1%83%D0%B7%D1%8B%D0%BA%D0%B0")).as_deref(), Some("/storage/emulated/0/Музыка"));
        // A memory card or USB drive: another volume, mounted under its id.
        assert_eq!(path(&format!("{ext}1A2B-3C4D%3AAlbums%2FJazz")).as_deref(), Some("/storage/1A2B-3C4D/Albums/Jazz"));
        assert_eq!(path(&format!("{ext}1A2B-3C4D%3A")).as_deref(), Some("/storage/1A2B-3C4D"));
        // A second user's internal storage.
        assert_eq!(tree_uri_path(&format!("{ext}primary%3AMusic"), Path::new("/storage/emulated/10")).unwrap(), Path::new("/storage/emulated/10/Music"));
        // The whole URI the picker may hand over, document part included.
        assert_eq!(path(&format!("{ext}primary%3AMusic/document/primary%3AMusic%2FLive")).as_deref(), Some("/storage/emulated/0/Music"));
        assert_eq!(path(&format!("{ext}home%3ASets")).as_deref(), Some("/storage/emulated/0/Documents/Sets"));
        // No path behind these: cloud drives, other apps, the Downloads provider's own ids.
        assert_eq!(path("content://com.google.android.apps.docs.storage/tree/acc%3D1%3Bdoc%3Dabc"), None);
        assert_eq!(path("content://com.android.providers.downloads.documents/tree/downloads"), None);
        assert_eq!(path("content://com.android.providers.downloads.documents/tree/msf%3A42"), None);
        // Nothing that climbs out of the volume or isn't a volume id.
        assert_eq!(path(&format!("{ext}primary%3A..%2F..%2Fdata")), None);
        assert_eq!(path(&format!("{ext}..%3AMusic")), None);
        assert_eq!(path(&format!("{ext}Music")), None);
        assert_eq!(path(&format!("{ext}primary%3AMusic%ZZ")), None);
        assert_eq!(path("/storage/emulated/0/Music"), None);
    }

    #[cfg(unix)]
    #[test]
    fn an_iphone_folder_follows_the_container() {
        let now = Path::new("/var/mobile/Containers/Data/Application/NEW-ID/Documents");
        assert_eq!(
            rebase("/var/mobile/Containers/Data/Application/OLD-ID/Documents/Podcasts/Weekly", now),
            "/var/mobile/Containers/Data/Application/NEW-ID/Documents/Podcasts/Weekly"
        );
        assert_eq!(rebase("/var/mobile/Containers/Data/Application/OLD-ID/Documents", now), now.to_string_lossy());
        // A folder of the user's own called Documents stays in the path.
        assert_eq!(
            rebase("/private/var/mobile/Containers/Data/Application/OLD-ID/Documents/Documents/x", now),
            "/var/mobile/Containers/Data/Application/NEW-ID/Documents/Documents/x"
        );
        // Already there, or not an app container at all: untouched.
        let same = "/var/mobile/Containers/Data/Application/NEW-ID/Documents/A";
        assert_eq!(rebase(same, now), same);
        assert_eq!(rebase("/Volumes/SSD/music/Podcasts", now), "/Volumes/SSD/music/Podcasts");
    }
}
