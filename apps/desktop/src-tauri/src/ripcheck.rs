//! Checking a CD rip against other people's rips of the same disc (see
//! hifi_engine::ripcheck), with the answers kept in ripcheck.json so an album checked once
//! shows its verdict at once next time.
//!
//! This is the one thing here that goes online, and only when asked to by the button: a
//! single request to the CUETools Database, carrying the disc's table of contents (where
//! each track starts, in CD frames) and nothing else — no file names, no tags, no audio.
//! The comparison itself happens on this device.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use hifi_engine::UserError;
use hifi_engine::ripcheck::{self, Report, Unfit};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

const LOOKUP: &str = "https://db.cue.tools/lookup2.php";
const SERVICE: &str = "CUETools Database";

/// What a check came to.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RipResult {
    /// Compared with what the database holds.
    #[serde(rename_all = "camelCase")]
    Checked {
        #[serde(flatten)]
        report: Report,
        /// When, in Unix seconds: the database grows, "unknown" may not stay so.
        checked_at: u64,
    },
    /// Not a CD rip that could be checked (nothing was sent anywhere).
    Unfit { why: Unfit },
}

#[derive(Serialize, Deserialize, Clone)]
struct Entry {
    /// The album's files as they were (paths, sizes and times, hashed).
    stamp: String,
    result: RipResult,
}

#[derive(Default, Clone)]
pub struct RipCheck {
    cache: Arc<Mutex<HashMap<String, Entry>>>,
    file: Option<PathBuf>,
    /// (done, total) tracks of the check that runs now; total 0: none runs.
    done: Arc<AtomicUsize>,
    total: Arc<AtomicUsize>,
}

/// The album's files as they are now: any of them changed, and the answer no longer counts.
fn stamp(paths: &[String]) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut eat = |bytes: &[u8]| {
        for b in bytes {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    };
    for p in paths {
        eat(p.as_bytes());
        if let Ok(m) = std::fs::metadata(hifi_engine::source_file(p)) {
            eat(&m.len().to_le_bytes());
            let t = m.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
            eat(&t.to_le_bytes());
        }
    }
    format!("{h:016x}")
}

impl RipCheck {
    pub fn load(app: &AppHandle) -> Self {
        let file = app.path().app_data_dir().ok().map(|d| d.join("ripcheck.json"));
        let cache = file.as_deref().and_then(hifi_engine::store::load_json).unwrap_or_default();
        Self { cache: Arc::new(Mutex::new(cache)), file, ..Default::default() }
    }

    fn cached(&self, paths: &[String]) -> Option<RipResult> {
        let key = paths.first()?;
        let s = stamp(paths);
        self.cache.lock().unwrap().get(key).filter(|e| e.stamp == s).map(|e| e.result.clone())
    }

    fn store(&self, paths: &[String], result: &RipResult) {
        let Some(key) = paths.first() else { return };
        let snapshot = {
            let mut cache = self.cache.lock().unwrap();
            cache.insert(key.clone(), Entry { stamp: stamp(paths), result: result.clone() });
            cache.clone()
        };
        if let Some(f) = &self.file {
            // Best effort: a lost write only means checking the album again.
            let _ = hifi_engine::store::save_json(f, &snapshot);
        }
    }
}

/// Ask the database for the rips it holds of the disc with this table of contents.
fn lookup(toc: &str) -> Result<Vec<ripcheck::Entry>, UserError> {
    let unreachable = |detail: String| UserError::ServiceUnreachable { service: SERVICE.into(), detail };
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_global(Some(Duration::from_secs(25)))
        .user_agent(concat!("BitSpring/", env!("CARGO_PKG_VERSION"), " (https://bitspring.app)"))
        .http_status_as_error(false)
        .build()
        .into();
    let mut resp = agent
        .get(LOOKUP)
        .query("version", "3")
        .query("ctdb", "1")
        // By the audio tracks alone: a disc's data track is not in its rip.
        .query("fuzzy", "1")
        // No titles asked for: only the checksums.
        .query("metadata", "none")
        .query("toc", toc)
        .call()
        .map_err(|e| unreachable(e.to_string()))?;
    match resp.status().as_u16() {
        200 => {
            let xml = resp.body_mut().with_config().limit(4 << 20).read_to_string().map_err(|e| unreachable(e.to_string()))?;
            Ok(ripcheck::parse_entries(&xml))
        }
        // The database's way of saying it doesn't know the disc.
        404 => Ok(Vec::new()),
        code => Err(unreachable(format!("HTTP {code}"))),
    }
}

/// What is known from an earlier check of this album (its tracks' paths, in disc order).
#[tauri::command(async)]
pub fn rip_cached(rip: State<'_, RipCheck>, paths: Vec<String>) -> Option<RipResult> {
    rip.cached(&paths)
}

/// (done, total) tracks read by the check that runs now.
#[tauri::command]
pub fn rip_progress(rip: State<'_, RipCheck>) -> Option<(usize, usize)> {
    let total = rip.total.load(Ordering::Acquire);
    (total > 0).then(|| (rip.done.load(Ordering::Acquire).min(total), total))
}

/// Check an album: read all of it, then ask the database once. Takes a while (the whole
/// album is decoded), and needs the network.
#[tauri::command]
pub async fn rip_check(rip: State<'_, RipCheck>, paths: Vec<String>) -> Result<RipResult, UserError> {
    let me = rip.inner().clone();
    let result = tauri::async_runtime::spawn_blocking(move || -> Result<RipResult, UserError> {
        me.done.store(0, Ordering::Release);
        me.total.store(paths.len(), Ordering::Release);
        let read = ripcheck::read(&paths, |n| me.done.store(n, Ordering::Release));
        me.total.store(0, Ordering::Release);
        let result = match read.map_err(|e| UserError::of(&e))? {
            Err(why) => RipResult::Unfit { why },
            Ok((toc, sums)) => {
                let entries = lookup(&toc.ctdb())?;
                let checked_at = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
                RipResult::Checked { report: ripcheck::compare(&sums, &entries), checked_at }
            }
        };
        me.store(&paths, &result);
        Ok(result)
    })
    .await;
    rip.total.store(0, Ordering::Release);
    result.map_err(|e| UserError::other(e))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use hifi_engine::ripcheck::Status;

    #[test]
    fn an_answer_counts_while_the_files_are_as_they_were() {
        let d = std::env::temp_dir().join(format!("bitspring-ripcheck-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let paths: Vec<String> = ["01.flac", "02.flac"].iter().map(|n| d.join(n).to_string_lossy().into_owned()).collect();
        for p in &paths {
            std::fs::write(p, b"audio").unwrap();
        }
        let rip = RipCheck { file: Some(d.join("ripcheck.json")), ..Default::default() };
        assert_eq!(rip.cached(&paths), None);
        let result = RipResult::Checked { report: Report { status: Status::Verified, matching: 9, known: 9, offset: 0, tracks: Vec::new() }, checked_at: 1_700_000_000 };
        rip.store(&paths, &result);
        assert_eq!(rip.cached(&paths), Some(result.clone()));
        // It is kept across launches…
        let saved: HashMap<String, Entry> = hifi_engine::store::load_json(&d.join("ripcheck.json")).unwrap();
        let again = RipCheck { cache: Arc::new(Mutex::new(saved)), ..Default::default() };
        assert_eq!(again.cached(&paths), Some(result));
        // …and dropped once a file of the album changed, or the album has other tracks.
        std::fs::write(&paths[1], b"other audio").unwrap();
        assert_eq!(rip.cached(&paths), None);
        assert_eq!(again.cached(&paths[..1]), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The real database, asked about a disc it has held for years (The National, "Boxer").
    /// Needs the network: `cargo test -p hifi-desktop -- --ignored the_database_answers`.
    #[test]
    #[ignore]
    fn the_database_answers() {
        let toc = "0:15417:31234:46099:59563:76004:94629:110569:125299:140219:155541:173771:194263:209037:227565";
        let entries = lookup(toc).unwrap();
        assert!(entries.iter().any(|e| e.crc32 == 0xadd9_0855 && e.confidence >= 9 && e.track_crcs.len() == 14), "{entries:?}");
        // A table of contents no disc has: known as "nothing", not as an error.
        assert_eq!(lookup("0:1000:2000:3001").unwrap(), Vec::new());
    }

    /// What the UI reads: the report's fields beside the kind.
    #[test]
    fn results_cross_to_the_ui_flat() {
        let r = RipResult::Checked { report: Report { status: Status::Partly, matching: 0, known: 12, offset: 0, tracks: vec![Some(true), Some(false)] }, checked_at: 5 };
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v, serde_json::json!({"kind": "checked", "status": "partly", "matching": 0, "known": 12, "offset": 0, "tracks": [true, false], "checkedAt": 5}));
        assert_eq!(serde_json::from_value::<RipResult>(v).unwrap(), r);
        let v = serde_json::to_value(RipResult::Unfit { why: Unfit::Frames }).unwrap();
        assert_eq!(v, serde_json::json!({"kind": "unfit", "why": "frames"}));
    }
}
