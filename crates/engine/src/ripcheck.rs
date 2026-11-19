//! Is this CD rip a good one? Answered the way rippers do: by comparing it with other
//! people's rips of the same disc, collected in the CUETools Database (CTDB). A scratch or
//! a failing drive damages one copy in its own way; when many independent rips have the very
//! same audio, that audio is what is on the disc.
//!
//! This module is the arithmetic, with no network in it: the disc's table of contents from
//! the tracks' lengths, the database's checksum of the audio (a CRC-32 that leaves out the
//! first and last ten CD frames, where drives differ in what they deliver), and the
//! comparison with what the database answered. The lookup itself is the app's, made only
//! when the listener asks for it.
//!
//! Pressings of one disc differ by a shift of a few hundred samples. So every checksum is
//! worked out for every shift up to five CD frames either way — not by reading the audio
//! 5879 times: the CRC of any stretch follows from the CRCs up to its two ends, and those
//! are kept, around each track boundary, while the audio goes by once.

use serde::{Deserialize, Serialize};

/// Samples (stereo pairs) in a CD frame: 1/75 s at 44.1 kHz.
pub const SECTOR: u64 = 588;
/// What the checksum leaves out at each end of the disc: ten frames.
const EDGE: u64 = 10 * SECTOR;
/// How far two pressings' audio may be shifted against each other and still be found.
pub const REACH: i64 = 5 * SECTOR as i64 - 1;
/// The prefix CRCs kept behind the reading point: enough to find the end of the checksummed
/// stretch once the disc's length is known (two edges and a reach back).
const RING: usize = 1 << 15;

/// CRC-32 as zip and FLAC's cousins use it (reflected 0xEDB88320).
const TABLE: [u32; 256] = {
    let mut t = [0u32; 256];
    let mut n = 0;
    while n < 256 {
        let mut c = n as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        t[n] = c;
        n += 1;
    }
    t
};

type Matrix = [u32; 32];

fn times(m: &Matrix, mut v: u32) -> u32 {
    let mut sum = 0;
    let mut i = 0;
    while v != 0 {
        if v & 1 != 0 {
            sum ^= m[i];
        }
        v >>= 1;
        i += 1;
    }
    sum
}

fn product(a: &Matrix, b: &Matrix) -> Matrix {
    std::array::from_fn(|n| times(a, b[n]))
}

/// What appending `len` zero bytes does to a CRC, as a matrix. With it, the CRC of a stretch
/// B that follows a stretch A is crc(AB) ^ zeros(len B)·crc(A).
fn zeros(mut len: u64) -> Matrix {
    // One zero bit, then squared up to one zero byte.
    let mut op: Matrix = std::array::from_fn(|n| if n == 0 { 0xEDB8_8320 } else { 1 << (n - 1) });
    for _ in 0..3 {
        op = product(&op, &op);
    }
    let mut result: Matrix = std::array::from_fn(|n| 1 << n);
    while len != 0 {
        if len & 1 != 0 {
            result = product(&op, &result);
        }
        op = product(&op, &op);
        len >>= 1;
    }
    result
}

/// The CRCs of everything up to each point within [`REACH`] of `at` (as far as the audio
/// goes on either side).
#[derive(Debug, Clone)]
struct Mark {
    at: u64,
    /// The offset of `states[0]` from `at`.
    lo: i64,
    states: Vec<u32>,
}

impl Mark {
    fn get(&self, offset: i64) -> Option<u32> {
        usize::try_from(offset - self.lo).ok().and_then(|i| self.states.get(i)).copied()
    }
}

/// Takes a disc's audio once, track by track, and keeps what the checksums are made of.
pub struct Summer {
    /// The CRC register over everything so far.
    crc: u32,
    count: u64,
    /// ring[k % RING] = the CRC of the first k samples.
    ring: Vec<u32>,
    start: Option<Mark>,
    /// One per finished track: around where it ended.
    ends: Vec<Mark>,
    /// Marks still taking the points after them.
    open: Vec<Open>,
}

#[derive(Clone, Copy)]
enum Open {
    Start,
    End(usize),
}

impl Default for Summer {
    fn default() -> Self {
        Self::new()
    }
}

impl Summer {
    pub fn new() -> Self {
        let mut ring = vec![0u32; RING];
        ring[0] = 0; // the CRC of nothing
        Self { crc: !0, count: 0, ring, start: None, ends: Vec::new(), open: Vec::new() }
    }

    fn mark_here(&self) -> Mark {
        let lo = -(REACH.min(self.count as i64));
        let states = (lo..=0).map(|o| self.ring[((self.count as i64 + o) as usize) % RING]).collect();
        Mark { at: self.count, lo, states }
    }

    /// Interleaved stereo samples of the current track, as decoded (±1.0 = full scale):
    /// 16-bit audio comes back from them exactly.
    pub fn push(&mut self, samples: &[f32]) {
        for pair in samples.as_chunks::<2>().0 {
            for &s in pair {
                let v = (s * 32_768.0).round().clamp(-32_768.0, 32_767.0) as i16;
                for b in v.to_le_bytes() {
                    self.crc = TABLE[((self.crc ^ b as u32) & 0xFF) as usize] ^ (self.crc >> 8);
                }
            }
            self.count += 1;
            let state = !self.crc;
            self.ring[(self.count as usize) % RING] = state;
            if !self.open.is_empty() {
                let count = self.count;
                let (start, ends) = (&mut self.start, &mut self.ends);
                self.open.retain(|o| {
                    let m = match o {
                        Open::Start => start.as_mut().expect("opened"),
                        Open::End(i) => &mut ends[*i],
                    };
                    m.states.push(state);
                    count < m.at + REACH as u64
                });
            }
            if self.count == EDGE {
                self.start = Some(self.mark_here());
                self.open.push(Open::Start);
            }
        }
    }

    /// The current track is over.
    pub fn end_track(&mut self) {
        self.ends.push(self.mark_here());
        self.open.push(Open::End(self.ends.len() - 1));
    }

    /// None: too short to be a CD (less than half a second of audio).
    pub fn finish(self) -> Option<Sums> {
        let n = self.count;
        if n < 4 * EDGE || self.ends.is_empty() {
            return None;
        }
        // Where the checksummed stretch ends: ten frames before the end, and back to a
        // whole number of ten frames.
        let end_at = n - EDGE - n % EDGE;
        let lo = -REACH;
        let states = (lo..=REACH).map(|o| self.ring[((end_at as i64 + o) as usize) % RING]).collect();
        let end = Mark { at: end_at, lo, states };
        let mut starts = vec![0u64];
        starts.extend(self.ends.iter().map(|m| m.at));
        let total = starts.pop().expect("at least one track");
        debug_assert_eq!(total, n);
        Some(Sums { starts, samples: n, start: self.start?, end, bounds: self.ends, disc_op: zeros((end_at - EDGE) * 4) })
    }
}

/// What a disc's checksums are made of (see [`Summer`]).
pub struct Sums {
    /// Where each track starts, in samples from the first one's start.
    starts: Vec<u64>,
    samples: u64,
    start: Mark,
    end: Mark,
    /// Around the end of each track.
    bounds: Vec<Mark>,
    disc_op: Matrix,
}

impl Sums {
    pub fn tracks(&self) -> usize {
        self.starts.len()
    }

    /// Each track's length in samples.
    pub fn lengths(&self) -> Vec<u64> {
        (0..self.tracks()).map(|i| self.starts.get(i + 1).copied().unwrap_or(self.samples) - self.starts[i]).collect()
    }

    /// The database's checksum of the whole disc, were the audio `offset` samples later.
    pub fn disc_crc(&self, offset: i64) -> Option<u32> {
        Some(self.end.get(offset)? ^ times(&self.disc_op, self.start.get(offset)?))
    }

    /// The shift at which the disc's checksum is `crc`: none (0) first, then the smallest.
    pub fn disc_offset(&self, crc: u32) -> Option<i64> {
        (0..=REACH).flat_map(|o| [o, -o]).find(|&o| self.disc_crc(o) == Some(crc))
    }

    /// The checksum of track `i` (from 0) at `offset`. The first track is counted from the
    /// disc's stretch on, the last one up to its end; at an offset those two ends stay put
    /// (`fixed_ends`, as CUETools counts them) or move with the rest.
    pub fn track_crc(&self, i: usize, offset: i64, fixed_ends: bool) -> Option<u32> {
        let last = self.tracks() - 1;
        let edge = |m: &Mark| Some((m.at as i64 + if fixed_ends { 0 } else { offset }, m.get(if fixed_ends { 0 } else { offset })?));
        let bound = |m: &Mark| Some((m.at as i64 + offset, m.get(offset)?));
        let (from, a) = if i == 0 { edge(&self.start)? } else { bound(&self.bounds[i - 1])? };
        let (to, b) = if i == last { edge(&self.end)? } else { bound(&self.bounds[i])? };
        (to > from).then(|| b ^ times(&zeros((to - from) as u64 * 4), a))
    }
}

/// A disc's table of contents, as far as its audio tracks tell: what the database is asked by.
#[derive(Debug, Clone, PartialEq)]
pub struct Toc {
    /// CD frames before the first track (0 on nearly every disc).
    pub pregap: u64,
    /// Each track's length in CD frames.
    pub sectors: Vec<u64>,
}

/// Why an album can't be a CD rip to check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Unfit {
    /// Not 16-bit 44.1 kHz stereo lossless.
    Format,
    /// A track isn't a whole number of CD frames long: cut, edited, or not from a CD.
    Frames,
    /// Too little audio, or more than a CD holds.
    Length,
}

impl Toc {
    /// From the tracks' lengths in samples.
    pub fn of(pregap: u64, lengths: &[u64]) -> Result<Toc, Unfit> {
        if lengths.is_empty() || lengths.len() > 99 {
            return Err(Unfit::Length);
        }
        if lengths.iter().any(|l| *l == 0 || l % SECTOR != 0) {
            return Err(Unfit::Frames);
        }
        let sectors: Vec<u64> = lengths.iter().map(|l| l / SECTOR).collect();
        // (A CD holds 80 minutes, a little more when overburnt.)
        if pregap + sectors.iter().sum::<u64>() > 100 * 60 * 75 {
            return Err(Unfit::Length);
        }
        Ok(Toc { pregap, sectors })
    }

    /// Track starts and the lead-out, in CD frames: "0:12617:40337:63075".
    pub fn ctdb(&self) -> String {
        let mut at = self.pregap;
        let mut out = Vec::with_capacity(self.sectors.len() + 1);
        for s in &self.sectors {
            out.push(at.to_string());
            at += s;
        }
        out.push(at.to_string());
        out.join(":")
    }
}

/// Read an album's audio — its tracks' library paths, in disc order — and sum it.
/// `progress` hears of each track done. Inner error: the album isn't a CD rip to check.
pub fn read(paths: &[String], progress: impl Fn(usize)) -> anyhow::Result<Result<(Toc, Sums), Unfit>> {
    let mut summer = Summer::new();
    for (i, path) in paths.iter().enumerate() {
        let track = crate::open_file(std::path::Path::new(path), crate::dsd::DsdMode::Pcm)?;
        if track.sample_rate != 44_100 || track.channels != 2 || track.bits_per_sample != Some(16) || track.lossy || track.dsd.is_some() {
            return Ok(Err(Unfit::Format));
        }
        crate::quality::read_all(&track, |s| summer.push(s))?;
        if let Some(e) = track.error() {
            anyhow::bail!(e);
        }
        summer.end_track();
        progress(i + 1);
    }
    // An image with a sheet says where its first track starts; separate files start the disc.
    let pregap = paths.first().and_then(|p| crate::cue::split(p).1).map_or(0, |r| r.start);
    let Some(sums) = summer.finish() else { return Ok(Err(Unfit::Length)) };
    Ok(Toc::of(pregap, &sums.lengths()).map(|toc| (toc, sums)))
}

/// One rip of a disc as the database holds it.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// How many times this very audio was submitted.
    pub confidence: u32,
    pub crc32: u32,
    /// Its audio tracks' lengths in CD frames (None: the entry's table of contents is unreadable).
    pub sectors: Option<Vec<u64>>,
    /// Per audio track; empty when the entry has none.
    pub track_crcs: Vec<u32>,
}

/// An entry's audio layout from its table of contents ("0:12617:…:-281585:333643": starts,
/// a data track marked "-", then the lead-out).
fn entry_sectors(toc: &str) -> Option<Vec<u64>> {
    let parts: Vec<(bool, u64)> = toc.split(':').map(|p| Some((p.starts_with('-'), p.trim_start_matches('-').parse().ok()?))).collect::<Option<_>>()?;
    let (&(_, leadout), tracks) = parts.split_last()?;
    let mut out = Vec::new();
    for (i, &(data, start)) in tracks.iter().enumerate() {
        if data {
            continue;
        }
        // Up to the next track — less the gap in front of a data session — or the lead-out.
        let end = match tracks.get(i + 1) {
            Some(&(true, next)) => next.checked_sub(11_400)?,
            Some(&(false, next)) => next,
            None => leadout,
        };
        out.push(end.checked_sub(start).filter(|l| *l > 0)?);
    }
    (!out.is_empty()).then_some(out)
}

/// The value of `name="…"` in an XML tag.
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let mut rest = tag;
    loop {
        let at = rest.find(name)?;
        let after = &rest[at + name.len()..];
        // The whole name, not the tail of a longer one.
        let whole = at == 0 || rest[..at].ends_with(|c: char| c.is_whitespace());
        if let (true, Some(v)) = (whole, after.strip_prefix("=\"")) {
            return v.split('"').next();
        }
        rest = after;
    }
}

/// The entries in the database's answer (its XML).
pub fn parse_entries(xml: &str) -> Vec<Entry> {
    xml.split("<entry")
        .skip(1)
        .filter_map(|rest| {
            let tag = rest.split('>').next()?;
            if !tag.starts_with(|c: char| c.is_whitespace()) {
                return None;
            }
            Some(Entry {
                confidence: attr(tag, "confidence")?.parse().ok()?,
                crc32: u32::from_str_radix(attr(tag, "crc32")?, 16).ok()?,
                sectors: attr(tag, "toc").and_then(entry_sectors),
                track_crcs: attr(tag, "trackcrcs").map(|t| t.split_whitespace().filter_map(|c| u32::from_str_radix(c, 16).ok()).collect()).unwrap_or_default(),
            })
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    /// The whole disc is the same audio as rips in the database.
    Verified,
    /// Some tracks are the same as in the database, the others differ: those were ripped
    /// with errors (or changed since).
    Partly,
    /// The database knows the disc, and nothing of this rip matches what it holds.
    Different,
    /// The database doesn't know this disc.
    Unknown,
}

/// What the comparison found.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub status: Status,
    /// Rips in the database with this very audio (the whole disc).
    pub matching: u32,
    /// Rips of this disc in the database, whatever their audio.
    pub known: u32,
    /// The shift in samples at which it matched: another pressing when not 0.
    pub offset: i64,
    /// Per track, when the disc doesn't match as a whole: the same as in the database's
    /// best-fitting rip (None: that rip has nothing to tell tracks by).
    pub tracks: Vec<Option<bool>>,
}

/// Compare a disc with the database's entries for its table of contents.
pub fn compare(sums: &Sums, entries: &[Entry]) -> Report {
    let ours: Vec<u64> = sums.lengths().iter().map(|l| l / SECTOR).collect();
    // The same audio layout (a lookup may also answer with discs that differ in a data track).
    let same_disc: Vec<&Entry> = entries.iter().filter(|e| e.sectors.as_ref().is_none_or(|s| *s == ours)).collect();
    let known: u32 = same_disc.iter().map(|e| e.confidence).sum();
    let nothing = |status| Report { status, matching: 0, known, offset: 0, tracks: Vec::new() };
    if same_disc.is_empty() {
        return nothing(Status::Unknown);
    }
    // The whole disc: every entry it matches counts, at the offset of the best of them.
    let whole: Vec<(&Entry, i64)> = same_disc.iter().filter_map(|e| Some((*e, sums.disc_offset(e.crc32)?))).collect();
    if let Some(&(_, offset)) = whole.iter().max_by_key(|(e, o)| (e.confidence, -o.abs())) {
        return Report { status: Status::Verified, matching: whole.iter().map(|(e, _)| e.confidence).sum(), known, offset, tracks: Vec::new() };
    }
    // Track by track, against the entry and shift that fit most tracks.
    let n = sums.tracks();
    let mut best: Option<(usize, u32, Vec<Option<bool>>)> = None;
    for e in same_disc.iter().filter(|e| e.track_crcs.len() == n) {
        for offset in (0..=REACH).flat_map(|o| [o, -o]) {
            // (Middle tracks decide whether this shift is worth a closer look.)
            let quick = (1..n.saturating_sub(1)).filter(|&i| sums.track_crc(i, offset, false) == Some(e.track_crcs[i])).count();
            if n > 2 && quick == 0 {
                continue;
            }
            let tracks: Vec<Option<bool>> =
                (0..n).map(|i| Some([false, true].iter().any(|&fixed| sums.track_crc(i, offset, fixed) == Some(e.track_crcs[i])))).collect();
            let hits = tracks.iter().filter(|t| **t == Some(true)).count();
            if hits > 0 && best.as_ref().is_none_or(|(h, c, _)| (hits, e.confidence) > (*h, *c)) {
                best = Some((hits, e.confidence, tracks));
            }
        }
    }
    match best {
        Some((_, _, tracks)) => Report { status: Status::Partly, matching: 0, known, offset: 0, tracks },
        None => nothing(Status::Different),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The CRC-32 of `bytes`, the plain way.
    fn crc32(bytes: &[u8]) -> u32 {
        !bytes.iter().fold(!0u32, |c, &b| TABLE[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8))
    }

    #[test]
    fn the_crc_is_the_usual_one() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
        // A stretch's CRC from the CRCs up to its two ends.
        let data: Vec<u8> = (0..5000u32).map(|i| (i * 7 + i / 13) as u8).collect();
        for (from, to) in [(0, 5000), (1, 4999), (1234, 1235), (4000, 5000)] {
            let via = crc32(&data[..to]) ^ times(&zeros((to - from) as u64), crc32(&data[..from]));
            assert_eq!(via, crc32(&data[from..to]), "{from}..{to}");
        }
    }

    /// Noise as a disc's audio: `sectors` CD frames of stereo samples, steady per `seed`.
    fn audio(seed: u32, sectors: u64) -> Vec<i16> {
        let mut x = seed.wrapping_mul(2_654_435_761).max(1);
        (0..sectors * SECTOR * 2)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                (x >> 16) as i16
            })
            .collect()
    }

    fn as_f32(pcm: &[i16]) -> Vec<f32> {
        pcm.iter().map(|&v| v as f32 / 32_768.0).collect()
    }

    fn bytes(pcm: &[i16]) -> Vec<u8> {
        pcm.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    /// The disc `pcm` cut into tracks of `sectors` frames, summed in uneven pieces.
    fn sums(pcm: &[i16], sectors: &[u64]) -> Sums {
        let mut s = Summer::new();
        let mut at = 0;
        for &n in sectors {
            let len = (n * SECTOR * 2) as usize;
            for piece in as_f32(&pcm[at..at + len]).chunks(2 * 4099) {
                s.push(piece);
            }
            s.end_track();
            at += len;
        }
        assert_eq!(at, pcm.len());
        s.finish().unwrap()
    }

    const DISC: [u64; 4] = [37, 52, 41, 33];

    /// The disc's and the tracks' checksums, worked out the plain way over the stretch each
    /// one covers, for audio shifted by `offset` samples.
    fn plain(pcm: &[i16], sectors: &[u64], offset: i64) -> (u32, Vec<u32>) {
        let n = pcm.len() as u64 / 2;
        let end = n - EDGE - n % EDGE;
        let b = bytes(pcm);
        let cut = |from: i64, to: i64| crc32(&b[(from * 4) as usize..(to * 4) as usize]);
        let mut starts = vec![0u64];
        for s in sectors {
            starts.push(starts.last().unwrap() + s * SECTOR);
        }
        let tracks = (0..sectors.len())
            .map(|i| {
                let from = if i == 0 { EDGE as i64 } else { starts[i] as i64 } + offset;
                let to = if i == sectors.len() - 1 { end as i64 } else { starts[i + 1] as i64 } + offset;
                cut(from, to)
            })
            .collect();
        (cut(EDGE as i64 + offset, end as i64 + offset), tracks)
    }

    #[test]
    fn every_checksum_at_every_shift_is_what_reading_the_stretch_gives() {
        let pcm = audio(1, DISC.iter().sum());
        let s = sums(&pcm, &DISC);
        assert_eq!(s.tracks(), 4);
        assert_eq!(s.lengths(), DISC.map(|n| n * SECTOR));
        for offset in [0, 1, -1, 588, -1777, REACH, -REACH] {
            let (disc, tracks) = plain(&pcm, &DISC, offset);
            assert_eq!(s.disc_crc(offset), Some(disc), "disc at {offset}");
            for (i, t) in tracks.iter().enumerate() {
                assert_eq!(s.track_crc(i, offset, false), Some(*t), "track {i} at {offset}");
            }
        }
        assert_eq!(s.disc_crc(REACH + 1), None);
        // At no shift the two ways of counting the ends are one.
        for i in 0..4 {
            assert_eq!(s.track_crc(i, 0, true), s.track_crc(i, 0, false));
        }
        // The stretch ends ten frames before the end, back to a whole ten frames: here
        // 163 frames of audio, so at frame 150.
        let n = pcm.len() as u64 / 2;
        assert_eq!((n / SECTOR, (n - EDGE - n % EDGE) / SECTOR), (163, 150));
    }

    fn entry(pcm: &[i16], sectors: &[u64], confidence: u32) -> Entry {
        let (crc32, track_crcs) = plain(pcm, sectors, 0);
        Entry { confidence, crc32, sectors: Some(sectors.to_vec()), track_crcs }
    }

    #[test]
    fn a_good_rip_is_verified_and_another_pressing_is_found_by_its_shift() {
        let pcm = audio(2, DISC.iter().sum());
        let s = sums(&pcm, &DISC);
        let db = [entry(&pcm, &DISC, 41), entry(&audio(3, DISC.iter().sum()), &DISC, 2)];
        let r = compare(&s, &db);
        assert_eq!((r.status, r.matching, r.known, r.offset), (Status::Verified, 41, 43, 0));
        // The database's rip is of a pressing 664 samples later: the same audio, shifted.
        let mut shifted = vec![0i16; 664 * 2];
        shifted.extend_from_slice(&pcm[..pcm.len() - 664 * 2]);
        let r = compare(&s, &[entry(&shifted, &DISC, 7)]);
        assert_eq!((r.status, r.matching, r.offset), (Status::Verified, 7, -664));
        // A disc the database doesn't hold — or holds only with another layout.
        assert_eq!(compare(&s, &[]).status, Status::Unknown);
        let other = Entry { sectors: Some(vec![37, 52, 41, 34]), ..db[0].clone() };
        assert_eq!(compare(&s, &[other]).status, Status::Unknown);
    }

    #[test]
    fn a_damaged_track_is_told_from_the_good_ones() {
        let good = audio(4, DISC.iter().sum());
        let db = [entry(&good, &DISC, 12)];
        // A click in the third track.
        let mut ours = good.clone();
        let at = ((37 + 52) * SECTOR * 2 + 5000) as usize;
        ours[at] = ours[at].wrapping_add(900);
        let r = compare(&sums(&ours, &DISC), &db);
        assert_eq!((r.status, r.matching, r.known), (Status::Partly, 0, 12));
        assert_eq!(r.tracks, [Some(true), Some(true), Some(false), Some(true)]);
        // Nothing in common with what the database has.
        let r = compare(&sums(&audio(5, DISC.iter().sum()), &DISC), &db);
        assert_eq!((r.status, r.known), (Status::Different, 12));
        // An entry without track checksums can't tell which track it is.
        let bare = Entry { track_crcs: Vec::new(), ..db[0].clone() };
        assert_eq!(compare(&sums(&ours, &DISC), &[bare]).status, Status::Different);
    }

    #[test]
    fn the_table_of_contents_comes_from_the_track_lengths() {
        let toc = Toc::of(0, &[12_617 * SECTOR, 27_720 * SECTOR, 22_738 * SECTOR]).unwrap();
        assert_eq!(toc.ctdb(), "0:12617:40337:63075");
        assert_eq!(Toc::of(33, &[100 * SECTOR, 50 * SECTOR]).unwrap().ctdb(), "33:133:183");
        // Cut or edited: not whole CD frames.
        assert_eq!(Toc::of(0, &[12_617 * SECTOR, 1000]), Err(Unfit::Frames));
        assert_eq!(Toc::of(0, &[]), Err(Unfit::Length));
        assert_eq!(Toc::of(0, &[500_000 * SECTOR]), Err(Unfit::Length));
    }

    #[test]
    fn the_database_s_answer_is_read() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ctdb xmlns="http://db.cuetools.net/ns/mmd-1.0#" xmlns:ext="http://db.cuetools.net/ns/ext-1.0#">
  <entry confidence="116" crc32="fa13993d" hasparity="http://p.cuetools.net/10453" id="10453" npar="16" stride="5880" syndrome="fuKJ2oIl=" toc="0:12617:40337:63075:93260:117965:151715:184190:215110:247305:-281585:333643" trackcrcs="49c620d1 1155246d 7c00490f" />
  <entry id="77" confidence="3" crc32="0000beef" toc="0:12617:40337:63075" />
  <entries note="not an entry" />
</ctdb>"#;
        let e = parse_entries(xml);
        assert_eq!(e.len(), 2);
        assert_eq!((e[0].confidence, e[0].crc32), (116, 0xfa13_993d));
        assert_eq!(e[0].track_crcs, [0x49c6_20d1, 0x1155_246d, 0x7c00_490f]);
        // Ten audio tracks; the last ends 11400 frames before the data track.
        let s = e[0].sectors.as_ref().unwrap();
        assert_eq!((s.len(), s[0], s[9]), (10, 12_617, 281_585 - 11_400 - 247_305));
        assert_eq!(e[1].sectors, Some(vec![12_617, 27_720, 22_738]));
        assert!(e[1].track_crcs.is_empty());
        assert!(parse_entries("Not Found").is_empty());
    }
}
