//! Blind comparison (ABX): "do I hear a difference?", answered by listening instead of by
//! looking at numbers.
//!
//! Two versions of the same music, A and B, and an X that is secretly one of them. The
//! listener switches between the three as often as they like and says which one X is, a
//! number of times over. Guessing gets half of them right; getting most of them right is
//! evidence of hearing a difference ([`p_value`] says how strong).
//!
//! For the test to mean anything, nothing but the sound itself may give X away. So the two
//! are prepared here, before the first note:
//!
//! * **One rate.** Files of different sample rates are brought to the higher of the two, so
//!   the DAC never switches (a switch clicks, and takes a moment that tells which is which).
//!   What is compared is then what the files hold — not how the DAC behaves at each rate.
//! * **Lined up in time.** Two editions of an album rarely start at the same instant. The
//!   later one's start is cut, to the sample, so a switch doesn't jump.
//! * **Equally loud.** The louder one is turned down to the other's loudness (EBU R128): a
//!   fraction of a decibel is heard as "better" long before it is heard as "louder".
//!
//! Each then sits in memory as a track of its own, and the player swaps one for the other
//! in mid-note ([`crate::output::Playback::swap`]).

use std::path::Path;
use std::sync::Arc;

use anyhow::{Result, bail};
use serde::Serialize;

use crate::decode::{Track, TrackMeta};
use crate::dsd::DsdMode;
use crate::dsp::loudness::{Basis, CEILING, Level, LevelMode, LevelSettings, Meter};
use crate::dsp::resample::{self, ResampleQuality, Resampler};
use crate::dsp::{DspSettings, Upsample};
use crate::UserError;

/// The most audio held per side: longer music is compared over its beginning.
const MAX_BYTES: usize = if cfg!(any(target_os = "ios", target_os = "android")) { 96 << 20 } else { 256 << 20 };

/// The chance of getting at least `correct` of `trials` right by guessing (each a coin
/// toss). Small = the listener very likely hears a difference; by convention, below 0.05.
pub fn p_value(correct: u32, trials: u32) -> f64 {
    if trials == 0 {
        return 1.0;
    }
    let correct = correct.min(trials);
    // C(n, k) / 2^n, summed from `correct` up; built up term by term.
    let mut term = 0.5f64.powi(trials as i32); // k = 0
    let mut sum = if correct == 0 { term } else { 0.0 };
    for k in 1..=trials {
        term *= (trials - k + 1) as f64 / k as f64;
        if k >= correct {
            sum += term;
        }
    }
    sum.min(1.0)
}

/// One of the two sides as it was found and as it plays.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Side {
    /// The file's own sample rate (a DSD file: the PCM rate it converts to).
    pub rate: u32,
    pub bits: Option<u32>,
    /// Brought to the other one's rate for the test.
    pub resampled: bool,
    /// Turned down by this much to match the other (0 or negative), dB.
    pub gain_db: f64,
}

/// What was done to make the two comparable — shown to the listener, so the test hides
/// nothing but X.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Prepared {
    /// The rate both play at.
    pub rate: u32,
    pub a: Side,
    pub b: Side,
    /// How much later B's music started than A's (negative: earlier), in milliseconds; the
    /// difference has been cut away.
    pub offset_ms: f64,
    /// How alike the two are once lined up: 1 = the same waveform, near 0 = different music.
    pub likeness: f64,
    /// Seconds of music compared.
    pub seconds: f64,
    /// The music is longer than what is compared.
    pub shortened: bool,
    /// Sample for sample the same: there is nothing to hear.
    pub identical: bool,
}

pub struct Pair {
    pub a: Arc<Track>,
    pub b: Arc<Track>,
    pub prepared: Prepared,
}

/// Decoded audio of one side.
struct Audio {
    rate: u32,
    channels: usize,
    layout: Vec<crate::channels::Speaker>,
    bits: Option<u32>,
    lossy: bool,
    samples: Vec<f32>,
    /// There was more than fits.
    shortened: bool,
}

impl Audio {
    fn frames(&self) -> usize {
        self.samples.len() / self.channels
    }
}

/// Decode `path` (a library path: a file or a CUE track), up to `max_bytes` of samples.
fn decode(path: &Path, max_bytes: usize) -> Result<Audio> {
    let track = crate::open_file(path, DsdMode::Pcm)?;
    let ch = track.channels;
    let max_frames = max_bytes / 4 / ch;
    let mut samples: Vec<f32> = Vec::new();
    let mut pos = 0;
    let mut shortened = false;
    loop {
        if pos >= max_frames {
            shortened = !(track.is_complete() && pos >= track.ready_frames());
            break;
        }
        pos = track.claim(pos);
        let s = track.slice(pos, (1 << 16).min(max_frames - pos));
        if s.is_empty() {
            if track.is_complete() && pos >= track.ready_frames() {
                break;
            }
            if !track.wait_for(pos + 1, std::time::Duration::from_secs(30)) {
                bail!("decoding stalled");
            }
            continue;
        }
        samples.extend_from_slice(s);
        pos += s.len() / ch;
    }
    if let (Some(e), true) = (track.error(), samples.is_empty()) {
        bail!(e);
    }
    Ok(Audio { rate: track.sample_rate, channels: ch, layout: track.layout.clone(), bits: track.bits_per_sample, lossy: track.lossy, samples, shortened })
}

/// `audio` at `rate`, through the player's own resampler.
fn at_rate(audio: &mut Audio, rate: u32) -> Result<()> {
    if audio.rate == rate {
        return Ok(());
    }
    if resample::phases(audio.rate, rate) > resample::MAX_PHASES {
        bail!(UserError::Resample { from: audio.rate, to: rate });
    }
    let mut r = Resampler::new(audio.rate, rate, audio.channels, ResampleQuality::HIGH);
    let mut out: Vec<f64> = Vec::with_capacity((audio.samples.len() as f64 * rate as f64 / audio.rate as f64) as usize + 4096);
    let mut input: Vec<f64> = Vec::new();
    for block in audio.samples.chunks(8192 * audio.channels) {
        input.clear();
        input.extend(block.iter().map(|&s| s as f64));
        r.process(&input, &mut out);
    }
    r.flush(&mut out);
    audio.samples = out.into_iter().map(|v| v as f32).collect();
    audio.rate = rate;
    Ok(())
}

fn mono(a: &Audio) -> Vec<f32> {
    let ch = a.channels;
    a.samples.chunks_exact(ch).map(|f| f.iter().sum::<f32>() / ch as f32).collect()
}

/// Every `d` samples averaged into one.
fn thinned(x: &[f32], d: usize) -> Vec<f32> {
    if d <= 1 {
        return x.to_vec();
    }
    x.chunks_exact(d).map(|c| c.iter().sum::<f32>() / d as f32).collect()
}

/// How alike `a[from..from + len]` and `b` shifted by `lag` are (b[i + lag] against a[i]):
/// their correlation, −1…1.
fn likeness_at(a: &[f32], b: &[f32], from: usize, len: usize, lag: isize) -> f64 {
    let (mut ab, mut aa, mut bb) = (0f64, 0f64, 0f64);
    let end = (from + len).min(a.len());
    for (i, &x) in a.iter().enumerate().take(end).skip(from) {
        let j = i as isize + lag;
        if j < 0 || j as usize >= b.len() {
            continue;
        }
        let (x, y) = (x as f64, b[j as usize] as f64);
        ab += x * y;
        aa += x * x;
        bb += y * y;
    }
    if aa > 0.0 && bb > 0.0 { ab / (aa * bb).sqrt() } else { 0.0 }
}

/// The lag within `around ± reach` at which `b` is most like `a` over the window.
fn best_lag(a: &[f32], b: &[f32], from: usize, len: usize, around: isize, reach: isize) -> (isize, f64) {
    let mut best = (around, f64::MIN);
    for lag in around - reach..=around + reach {
        let l = likeness_at(a, b, from, len, lag);
        if l > best.1 {
            best = (lag, l);
        }
    }
    best
}

/// Where the loudest stretch of `len` samples starts (looked for in a few places), so the
/// two are lined up by music and not by a quiet intro.
fn loud_window(x: &[f32], len: usize) -> usize {
    if x.len() <= len {
        return 0;
    }
    let energy = |from: usize| x[from..from + len].iter().map(|&v| (v as f64).powi(2)).sum::<f64>();
    (1..=8).map(|k| (x.len() - len) * k / 9).max_by(|&p, &q| energy(p).total_cmp(&energy(q))).unwrap_or(0)
}

/// How many frames later B's music is than A's (b[i + lag] is a[i]), and how alike they are
/// then. Found coarsely over thinned-out copies first (within ±`reach_secs`), then to the
/// sample.
fn line_up(a: &Audio, b: &Audio, reach_secs: f64) -> (isize, f64) {
    let rate = a.rate as usize;
    let (ma, mb) = (mono(a), mono(b));
    // About 1 kHz: thirty seconds of it against every lag in reach.
    let d1 = (rate / 1000).max(1);
    let (a1, b1) = (thinned(&ma, d1), thinned(&mb, d1));
    let len1 = (30 * rate / d1).min(a1.len());
    let from1 = loud_window(&a1, len1);
    let (lag1, _) = best_lag(&a1, &b1, from1, len1, 0, (reach_secs * rate as f64 / d1 as f64) as isize);
    // About 8 kHz, around that.
    let d2 = (rate / 8000).max(1);
    let (a2, b2) = (thinned(&ma, d2), thinned(&mb, d2));
    let len2 = (5 * rate / d2).min(a2.len());
    let from2 = (from1 * d1 / d2).min(a2.len().saturating_sub(len2));
    let (lag2, _) = best_lag(&a2, &b2, from2, len2, lag1 * d1 as isize / d2 as isize, (d1 / d2 + 2) as isize);
    // Every sample, around that.
    let len3 = (2 * rate).min(ma.len());
    let from3 = (from2 * d2).min(ma.len().saturating_sub(len3));
    let (lag, _) = best_lag(&ma, &mb, from3, len3, lag2 * d2 as isize, d2 as isize + 2);
    // Over a longer stretch than the one it was found by.
    let wide = (20 * rate).min(ma.len());
    let from = from3.min(ma.len().saturating_sub(wide));
    (lag, likeness_at(&ma, &mb, from, wide, lag))
}

fn loudness(a: &Audio) -> (Option<f64>, f64) {
    let mut m = Meter::new(a.rate, a.channels, &a.layout);
    for block in a.samples.chunks(a.channels * 65_536) {
        m.push(block);
    }
    let measured = m.finish();
    (measured.lufs, measured.peak)
}

fn scale(a: &mut Audio, gain: f64) {
    if gain != 1.0 {
        a.samples.iter_mut().for_each(|s| *s = (*s as f64 * gain) as f32);
    }
}

fn db(gain: f64) -> f64 {
    if gain == 1.0 { 0.0 } else { 20.0 * gain.log10() }
}

/// Line the two up, cut them to the same length and bring them to the same loudness.
/// `a_before`, `b_before`: what each was before it came to the common rate. `b_to_a`: B is
/// brought to A's loudness whichever is louder (B was rendered with room to spare); else
/// the louder of the two comes down.
fn finish(mut a: Audio, mut b: Audio, a_before: (u32, Option<u32>), b_before: (u32, Option<u32>), reach_secs: f64, b_to_a: bool) -> Pair {
    let rate = a.rate;
    let ch = a.channels;
    let (lag, likeness) = line_up(&a, &b, reach_secs);
    // B later: its start goes. B earlier: A's.
    if lag > 0 {
        b.samples.drain(..(lag as usize * ch).min(b.samples.len()));
    } else if lag < 0 {
        a.samples.drain(..((-lag) as usize * ch).min(a.samples.len()));
    }
    let frames = a.frames().min(b.frames());
    a.samples.truncate(frames * ch);
    b.samples.truncate(frames * ch);
    let identical = a.samples == b.samples;
    // The louder one down to the other. (A hundredth of a decibel is no difference, and
    // leaving both alone keeps both exactly as they are in their files.)
    let ((la, pa), (lb, pb)) = (loudness(&a), loudness(&b));
    let (mut ga, mut gb) = match (la, lb) {
        (Some(la), Some(lb)) if (la - lb).abs() >= 0.01 && !identical => {
            if la > lb && !b_to_a { (10f64.powf((lb - la) / 20.0), 1.0) } else { (1.0, 10f64.powf((la - lb) / 20.0)) }
        }
        _ => (1.0, 1.0),
    };
    // Neither may clip (processing can raise peaks): both down together, if it comes to that.
    let peak = (pa * ga).max(pb * gb);
    if peak > 1.0 {
        ga *= CEILING / peak;
        gb *= CEILING / peak;
    }
    scale(&mut a, ga);
    scale(&mut b, gb);
    let prepared = Prepared {
        rate,
        a: Side { rate: a_before.0, bits: a_before.1, resampled: a_before.0 != rate, gain_db: db(ga) },
        b: Side { rate: b_before.0, bits: b_before.1, resampled: b_before.0 != rate, gain_db: db(gb) },
        offset_ms: lag as f64 * 1000.0 / rate as f64,
        likeness,
        seconds: frames as f64 / rate as f64,
        shortened: a.shortened || b.shortened,
        identical,
    };
    let track = |x: Audio, touched: bool| {
        // (Untouched samples keep the file's bit depth, so its bit-perfect path stands.)
        let mut t = Track::from_samples(rate, ch, if touched { Some(24) } else { x.bits }, &x.samples, TrackMeta::default());
        t.layout = x.layout;
        Arc::new(t)
    };
    let (a_touched, b_touched) = (prepared.a.resampled || ga != 1.0, prepared.b.resampled || gb != 1.0);
    Pair { a: track(a, a_touched), b: track(b, b_touched), prepared }
}

/// Two files of the same music (a hi-res edition and the CD, a remaster and the original).
pub fn of_files(a: &Path, b: &Path) -> Result<Pair> {
    let mut a = decode(a, MAX_BYTES)?;
    let mut b = decode(b, MAX_BYTES)?;
    if a.channels != b.channels || a.layout != b.layout {
        bail!(UserError::AbxChannels);
    }
    let (a_before, b_before) = ((a.rate, a.bits), (b.rate, b.bits));
    // The higher rate: the better file stays as it is.
    let rate = a.rate.max(b.rate);
    // What doesn't fit at that rate is left off the end.
    for x in [&mut a, &mut b] {
        let fits = (MAX_BYTES / 4 / x.channels) as u64 * x.rate as u64 / rate as u64;
        if x.frames() as u64 > fits {
            x.samples.truncate(fits as usize * x.channels);
            x.shortened = true;
        }
        at_rate(x, rate)?;
    }
    // Editions can differ by seconds of silence at the start.
    Ok(finish(a, b, a_before, b_before, 5.0, false))
}

/// One file as it is (A) and as the processing in `dsp` makes it (B): equaliser, crossfeed,
/// convolution, the lossy enhancers. Not upsampling — the DAC would have to switch rates
/// between the two — and not levelling, which the test's own level matching would undo.
pub fn of_processing(path: &Path, dsp: &DspSettings) -> Result<Pair> {
    let dsp = DspSettings { upsample: Upsample::Off, eq_bypass: false, level: LevelSettings { mode: LevelMode::Off, headroom: true }, ..dsp.clone() };
    let a = decode(path, MAX_BYTES)?;
    if !(dsp.reshapes() || (a.lossy && dsp.lossy.any())) {
        bail!(UserError::AbxNothing);
    }
    let before = (a.rate, a.bits);
    // Through the chain 12 dB down, so a boost in it has room; the level matching brings
    // it back up to A.
    let source = {
        let mut t = Track::from_samples(a.rate, a.channels, a.bits, &a.samples, TrackMeta::default());
        t.layout = a.layout.clone();
        t.lossy = a.lossy;
        t.set_level(Some(Level { gain: 0.25, basis: Basis::Peak, limited: false }));
        Arc::new(t)
    };
    let processed = crate::output::render_offline(source, &dsp)?;
    let b = Audio { rate: a.rate, channels: a.channels, layout: a.layout.clone(), bits: Some(24), lossy: a.lossy, samples: processed, shortened: a.shortened };
    // The chain's own delay is a fraction of a second at most. A stays as it is in the file:
    // it is B that comes up to it.
    let mut pair = finish(a, b, before, before, 1.0, true);
    // B is the same file: what was done to it is the processing, not a conversion.
    pair.prepared.b.resampled = false;
    Ok(pair)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::encode::{Codec, Encoder, Format};
    use std::path::PathBuf;

    #[test]
    fn the_chance_of_guessing_that_well() {
        assert!((p_value(16, 16) - 1.0 / 65_536.0).abs() < 1e-12);
        assert!((p_value(12, 16) - 0.0384).abs() < 0.0001);
        assert!((p_value(9, 10) - 11.0 / 1024.0).abs() < 1e-12);
        assert!((p_value(8, 16) - 0.598).abs() < 0.001);
        // Half right or worse is what guessing gives; nothing answered says nothing.
        assert_eq!(p_value(0, 12), 1.0);
        assert_eq!(p_value(0, 0), 1.0);
        assert!(p_value(5, 5) < 0.05 && p_value(4, 5) > 0.05);
    }

    /// Something like music for lining up: tones that swell and fade at their own paces, so
    /// no stretch of it repeats.
    fn music(rate: u32, secs: f64) -> Vec<f64> {
        const TONES: [(f64, f64, f64); 6] = [(220.0, 0.31, 0.0), (331.7, 0.47, 1.0), (517.3, 0.73, 2.0), (1234.5, 1.1, 3.0), (3301.1, 1.7, 4.0), (7919.3, 2.3, 5.0)];
        (0..(rate as f64 * secs) as usize)
            .map(|i| {
                let t = i as f64 / rate as f64;
                TONES.iter().map(|&(f, pace, phase)| 0.1 * (0.5 + 0.5 * (std::f64::consts::TAU * pace * t + phase).sin()) * (std::f64::consts::TAU * f * t).sin()).sum()
            })
            .collect()
    }

    fn tmpdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("bitspring-abx-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A stereo FLAC of `mono` (the right channel a little lower, so the two differ).
    fn flac(dir: &Path, name: &str, rate: u32, bits: u32, mono: &[f64]) -> PathBuf {
        let full = ((1u32 << (bits - 1)) - 1) as f64;
        let mut enc = Encoder::new(Format { codec: Codec::Flac, rate, channels: 2, bits: bits as u16 });
        let mut bytes = enc.header();
        let samples: Vec<i32> = mono.iter().flat_map(|&v| [(v * full).round() as i32, (v * 0.8 * full).round() as i32]).collect();
        for block in samples.chunks(4096 * 2) {
            enc.encode(block, &mut bytes);
        }
        let p = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }

    fn samples(t: &Track) -> Vec<f32> {
        t.to_vec()
    }

    fn lufs(t: &Track) -> f64 {
        let mut m = Meter::new(t.sample_rate, t.channels, &t.layout);
        m.push(&samples(t));
        m.finish().lufs.unwrap()
    }

    /// Another edition of the same music: it starts later and is mastered louder. Lined up
    /// to the sample and brought to the same loudness, the two are the same sound.
    #[test]
    fn a_later_and_louder_edition_is_lined_up_and_levelled() {
        let d = tmpdir("editions");
        let m = music(44_100, 12.0);
        let a = flac(&d, "a.flac", 44_100, 16, &m[..10 * 44_100]);
        // B: 1234 samples of silence first, then the same music twice as loud.
        let later: Vec<f64> = std::iter::repeat_n(0.0, 1234).chain(m.iter().map(|v| v * 2.0)).collect();
        let b = flac(&d, "b.flac", 44_100, 16, &later);
        let pair = of_files(&a, &b).unwrap();
        let p = &pair.prepared;
        assert_eq!((p.rate, p.a.resampled, p.b.resampled), (44_100, false, false));
        assert!((p.offset_ms - 1234.0 * 1000.0 / 44_100.0).abs() < 0.001, "{}", p.offset_ms);
        assert!(p.likeness > 0.999, "{}", p.likeness);
        assert!(!p.identical && !p.shortened);
        // The louder one came down by its 6.02 dB; the other is as in its file.
        assert_eq!(p.a.gain_db, 0.0);
        assert!((p.b.gain_db + 6.02).abs() < 0.05, "{}", p.b.gain_db);
        assert_eq!(pair.a.bits_per_sample, Some(16));
        assert_eq!(pair.b.bits_per_sample, Some(24));
        let (sa, sb) = (samples(&pair.a), samples(&pair.b));
        assert_eq!(sa.len(), sb.len());
        assert!((p.seconds - 10.0).abs() < 0.001);
        let worst = sa.iter().zip(&sb).map(|(x, y)| (x - y).abs()).fold(0f32, f32::max);
        assert!(worst < 3.0 / 32_768.0, "the two differ by {worst}");
        // The other way round: A is the later one.
        let back = of_files(&b, &a).unwrap().prepared;
        assert!((back.offset_ms + 1234.0 * 1000.0 / 44_100.0).abs() < 0.001, "{}", back.offset_ms);
        assert!((back.a.gain_db + 6.02).abs() < 0.05 && back.b.gain_db == 0.0);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A hi-res file against the CD edition: both play at the higher rate, the hi-res one
    /// untouched, so the DAC never switches.
    #[test]
    fn files_of_two_rates_meet_at_the_higher_one() {
        let d = tmpdir("rates");
        let hi = flac(&d, "hi.flac", 88_200, 24, &music(88_200, 8.0));
        let cd = flac(&d, "cd.flac", 44_100, 16, &music(44_100, 8.0));
        let pair = of_files(&hi, &cd).unwrap();
        let p = &pair.prepared;
        assert_eq!((p.rate, p.a.rate, p.b.rate), (88_200, 88_200, 44_100));
        assert!(!p.a.resampled && p.b.resampled);
        assert_eq!((pair.a.sample_rate, pair.b.sample_rate), (88_200, 88_200));
        assert_eq!((pair.a.bits_per_sample, p.a.gain_db), (Some(24), 0.0));
        assert!(p.likeness > 0.999, "{}", p.likeness);
        // The same music at the same level: nothing to match, nothing to shift but the
        // resampler's own fraction of a millisecond.
        assert!(p.offset_ms.abs() < 2.0 && p.b.gain_db.abs() < 0.05, "{p:?}");
        assert_eq!(pair.a.frames(), pair.b.frames());
        // The hi-res file's samples are the file's own.
        let file = crate::open_file(&hi, DsdMode::Pcm).unwrap();
        file.wait_for(usize::MAX, std::time::Duration::from_secs(10));
        let (own, played) = (file.to_vec(), samples(&pair.a));
        let skipped = own.len() - played.len();
        assert!(skipped % 2 == 0 && own[own.len() - played.len()..] == played[..] || own[..played.len()] == played[..], "A was changed");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_same_file_twice_is_told_as_such() {
        let d = tmpdir("same");
        let a = flac(&d, "a.flac", 48_000, 24, &music(48_000, 6.0));
        let p = of_files(&a, &a).unwrap().prepared;
        assert!(p.identical && p.offset_ms == 0.0 && p.likeness > 0.9999);
        assert_eq!((p.a.gain_db, p.b.gain_db), (0.0, 0.0));
        // Mono against stereo can't be switched between.
        let mut enc = Encoder::new(Format { codec: Codec::Flac, rate: 48_000, channels: 1, bits: 16 });
        let mut bytes = enc.header();
        for _ in 0..12 {
            enc.encode(&[1000; 4096], &mut bytes);
        }
        let mono = d.join("mono.flac");
        std::fs::write(&mono, bytes).unwrap();
        let e = of_files(&a, &mono).err().unwrap();
        assert_eq!(UserError::of(&e), UserError::AbxChannels);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The processing against the file itself: A is the file as it is, B what the chain
    /// makes of it, at A's loudness.
    #[test]
    fn processing_is_compared_at_the_same_loudness() {
        use crate::dsp::crossfeed::CrossfeedLevel;
        use crate::dsp::eq::{Band, EqPreset, FilterKind};
        let d = tmpdir("processing");
        let path = flac(&d, "a.flac", 44_100, 16, &music(44_100, 8.0));
        // Nothing switched on: nothing to compare.
        let e = of_processing(&path, &DspSettings::default()).err().unwrap();
        assert_eq!(UserError::of(&e), UserError::AbxNothing);
        // (Upsampling and levelling alone don't count: they are left out of the test.)
        let only_rate = DspSettings { upsample: Upsample::UpTo(192_000.0), level: LevelSettings { mode: LevelMode::Track, headroom: false }, ..Default::default() };
        assert_eq!(UserError::of(&of_processing(&path, &only_rate).err().unwrap()), UserError::AbxNothing);
        let boost = EqPreset { bands: vec![Band { kind: FilterKind::Peak, freq: 300.0, gain: 9.0, q: 1.0, ..Default::default() }], ..Default::default() };
        for dsp in [
            DspSettings { crossfeed: Some(CrossfeedLevel::DEFAULT), ..Default::default() },
            DspSettings { eq: Some(boost), upsample: Upsample::UpTo(192_000.0), ..Default::default() },
        ] {
            let pair = of_processing(&path, &dsp).unwrap();
            let p = &pair.prepared;
            assert_eq!((p.rate, p.a.resampled, p.b.resampled), (44_100, false, false));
            assert!(!p.identical && p.likeness > 0.5, "{p:?}");
            assert!(p.offset_ms.abs() < 50.0, "{p:?}");
            assert!((lufs(&pair.a) - lufs(&pair.b)).abs() < 0.1, "{} vs {}", lufs(&pair.a), lufs(&pair.b));
            assert_eq!(pair.a.frames(), pair.b.frames());
            let peak = samples(&pair.b).iter().fold(0f32, |m, v| m.max(v.abs()));
            assert!(peak <= 1.0, "B clips: {peak}");
            // A is the file itself unless B needed the room.
            if p.a.gain_db == 0.0 {
                assert_eq!(pair.a.bits_per_sample, Some(16));
            }
        }
        let _ = std::fs::remove_dir_all(&d);
    }
}
