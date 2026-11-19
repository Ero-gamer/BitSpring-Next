//! Volume levelling (ReplayGain, EBU R128) and headroom for peaks between the samples.
//!
//! Both come down to one gain per track, set before it plays and held for its whole length —
//! never a compressor or a limiter, so the dynamics inside a track (and, in album mode, between
//! the tracks of an album) stay as mastered.
//!
//! * **Levelling** brings every track, or every album, to the same loudness: −18 LUFS, the
//!   ReplayGain 2.0 reference. What the gain is comes from the file's own tags when it has
//!   them, else from measuring the audio the way EBU R128 / ITU-R BS.1770 describe.
//! * **Peak protection.** A gain that would push the track's true peak past full scale is cut
//!   back to what fits, instead of clipping the signal.
//! * **Headroom** (without levelling): upsampling and filters rebuild the waveform between the
//!   samples, where a loud master can peak above full scale. The track is lowered by exactly
//!   what its true peak overshoots, and not at all when it doesn't.
//!
//! All of it is off by default; a player with none of it on takes the bit-perfect path.

use std::f64::consts::PI;

use serde::{Deserialize, Serialize};

use crate::channels::Speaker;
use crate::decode::Track;

/// Loudness every track (or album) is brought to: ReplayGain 2.0's reference.
pub const TARGET_LUFS: f64 = -18.0;
/// Highest true peak after the gain: −0.3 dBFS (the same ceiling the lossy headroom uses).
pub const CEILING: f64 = 0.966;
/// Version of the measurement: results from older rules are measured again.
/// 2: dynamic range (DR) and loudness range (LRA) are measured too.
pub const RULES_VERSION: u32 = 2;
/// The oldest rules whose loudness and peak still stand (what came later only added figures):
/// levelling goes on by those results while they wait to be measured again.
pub const LOUDNESS_SINCE: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LevelMode {
    #[default]
    Off,
    /// Every track equally loud (playlists, shuffle).
    Track,
    /// Every album equally loud; the tracks of one album keep their levels relative to each
    /// other.
    Album,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LevelSettings {
    pub mode: LevelMode,
    /// Without levelling, when other processing runs: lower a track whose true peak would
    /// overshoot full scale.
    pub headroom: bool,
}

/// How loud something is, as what levelling needs: the gain that brings it to the target, and
/// its peak.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Loudness {
    /// dB to add to reach [`TARGET_LUFS`].
    pub gain_db: f64,
    /// Highest |sample| (1.0 = full scale), true peak when we measured it. None: a tag gave
    /// the gain without a peak.
    pub peak: Option<f64>,
}

/// What is known about a track before it plays.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Known {
    pub track: Option<Loudness>,
    pub album: Option<Loudness>,
}

/// What a gain is based on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Basis {
    Track,
    Album,
    /// No levelling: only headroom for the track's peak.
    Peak,
    /// Levelling is on, but nothing is known about this track yet: it plays as it is.
    Unknown,
}

/// The gain one track plays at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Level {
    /// Linear gain.
    pub gain: f64,
    pub basis: Basis,
    /// The gain was cut back to keep the peak under full scale.
    pub limited: bool,
}

impl Level {
    pub fn gain_db(&self) -> f64 {
        20.0 * self.gain.log10()
    }
}

/// Decide the gain for a track. `processing`: other processing (upsampling, EQ, convolution,
/// crossfeed) will run — what `headroom` is for. None = the track plays untouched.
pub fn resolve(settings: &LevelSettings, processing: bool, known: &Known) -> Option<Level> {
    let fit = |gain: f64, peak: Option<f64>| {
        // A tag without a peak: assume the track reaches full scale.
        let peak = peak.unwrap_or(1.0).max(1e-9);
        if gain * peak > CEILING { (CEILING / peak, true) } else { (gain, false) }
    };
    let (pick, basis) = match settings.mode {
        LevelMode::Off => {
            if !settings.headroom || !processing {
                return None;
            }
            // The album's peak when known, so its tracks keep their relative levels.
            let peak = known.album.and_then(|a| a.peak).or(known.track.and_then(|t| t.peak))?;
            let (gain, limited) = fit(1.0, Some(peak));
            return limited.then_some(Level { gain, basis: Basis::Peak, limited });
        }
        LevelMode::Track => match (known.track, known.album) {
            (Some(t), _) => (t, Basis::Track),
            (None, Some(a)) => (a, Basis::Album),
            (None, None) => return Some(Level { gain: 1.0, basis: Basis::Unknown, limited: false }),
        },
        LevelMode::Album => match (known.album, known.track) {
            (Some(a), _) => (a, Basis::Album),
            (None, Some(t)) => (t, Basis::Track),
            (None, None) => return Some(Level { gain: 1.0, basis: Basis::Unknown, limited: false }),
        },
    };
    let (gain, limited) = fit(10f64.powf(pick.gain_db / 20.0), pick.peak);
    Some(Level { gain, basis, limited })
}

// ---------------------------------------------------------------------------------------------
// Tags
// ---------------------------------------------------------------------------------------------

/// ReplayGain / R128 values found in a file's tags.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TagGain {
    pub track_db: Option<f64>,
    pub track_peak: Option<f64>,
    pub album_db: Option<f64>,
    pub album_peak: Option<f64>,
}

impl TagGain {
    pub fn is_empty(&self) -> bool {
        self.track_db.is_none() && self.album_db.is_none()
    }

    pub fn known(&self) -> Known {
        Known {
            track: self.track_db.map(|gain_db| Loudness { gain_db, peak: self.track_peak }),
            album: self.album_db.map(|gain_db| Loudness { gain_db, peak: self.album_peak }),
        }
    }

    /// A CUE track read from an image: the file's "track" values describe the whole image,
    /// so they are what the album's are.
    pub fn of_image(self) -> Self {
        Self {
            track_db: None,
            track_peak: None,
            album_db: self.album_db.or(self.track_db),
            album_peak: self.album_peak.or(self.track_peak),
        }
    }
}

/// "-6.48 dB" → −6.48. Refuses nonsense (a gain beyond ±60 dB is a broken tag).
pub fn parse_gain(text: &str) -> Option<f64> {
    let t = text.trim();
    let t = t.strip_suffix("dB").or_else(|| t.strip_suffix("db")).or_else(|| t.strip_suffix("DB")).unwrap_or(t);
    t.trim().replace(',', ".").parse::<f64>().ok().filter(|v| v.is_finite() && v.abs() <= 60.0)
}

/// "0.988553" → 0.988553 (linear, 1.0 = full scale).
pub fn parse_peak(text: &str) -> Option<f64> {
    text.trim().replace(',', ".").parse::<f64>().ok().filter(|v| v.is_finite() && *v > 0.0 && *v <= 16.0)
}

/// An R128_TRACK_GAIN / R128_ALBUM_GAIN tag: 1/256 dB steps relative to −23 LUFS, turned into
/// a gain towards our target.
pub fn parse_r128(text: &str) -> Option<f64> {
    let q = text.trim().parse::<i32>().ok().filter(|v| v.abs() <= 60 * 256)?;
    Some(q as f64 / 256.0 + (TARGET_LUFS + 23.0))
}

// ---------------------------------------------------------------------------------------------
// Measuring
// ---------------------------------------------------------------------------------------------

/// Gating blocks are sorted into bins this wide, from [`HIST_FLOOR`] up.
const HIST_STEP: f64 = 1.0;
/// The absolute gate of BS.1770: blocks below −70 LUFS never count.
const HIST_FLOOR: f64 = -70.0;
const HIST_BINS: usize = 80;

/// What measuring one track gives. Small enough to keep for every file of a library, and
/// enough to work out an album's loudness later without decoding its tracks again (the
/// histogram holds the gating blocks).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Measured {
    /// Integrated loudness, LUFS. None: silence (nothing above the absolute gate).
    pub lufs: Option<f64>,
    /// True peak, linear.
    pub peak: f64,
    /// Mean energy of all the blocks in `hist` (before the relative gate): pins down what the
    /// histogram's 1 LU steps blur.
    pub energy: f64,
    /// Bin of the first count in `hist`.
    pub hist_from: u16,
    /// Gating blocks (400 ms) per 1 LU of loudness.
    pub hist: Vec<u32>,
    /// Dynamic range, the "DR" figure of the dynamic range databases (see [`DrMeter`]), in
    /// dB and not rounded. None: not measured (older rules), or nothing but silence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dr: Option<f64>,
    /// Loudness range (LRA, EBU Tech 3342), in LU. None: not measured, or too short or quiet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lra: Option<f64>,
    /// Bin of the first count in `st`.
    #[serde(default)]
    pub st_from: u16,
    /// Short-term loudness readings (3 s windows, one every 100 ms) per 1 LU: what an
    /// album's loudness range is worked out from.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub st: Vec<u32>,
}

impl Measured {
    pub fn loudness(&self) -> Option<Loudness> {
        self.lufs.map(|l| Loudness { gain_db: TARGET_LUFS - l, peak: Some(self.peak) })
    }

    /// The DR figure as it is quoted: a whole number.
    pub fn dr_rounded(&self) -> Option<i32> {
        self.dr.map(|d| d.round() as i32)
    }
}

/// An album's DR: the mean of its tracks' figures, each rounded first — the way the DR
/// databases list albums. None when no track has one.
pub fn album_dr<'a>(tracks: impl IntoIterator<Item = &'a Measured>) -> Option<i32> {
    let (sum, n) = tracks.into_iter().filter_map(Measured::dr_rounded).fold((0i64, 0i64), |(s, n), d| (s + d as i64, n + 1));
    (n > 0).then(|| (sum as f64 / n as f64).round() as i32)
}

/// An album's loudness range: its tracks' short-term readings taken as one programme.
/// Their 1 LU bins put it within about half an LU of measuring the album in one go.
pub fn album_lra<'a>(tracks: impl IntoIterator<Item = &'a Measured>) -> Option<f64> {
    let mut count = [0f64; HIST_BINS];
    for m in tracks {
        for (i, &n) in m.st.iter().enumerate() {
            if let Some(c) = count.get_mut(m.st_from as usize + i) {
                *c += n as f64;
            }
        }
    }
    loudness_range(&count, HIST_STEP)
}

/// EBU Tech 3342 over short-term readings above the absolute gate, binned `step` LU wide from
/// [`HIST_FLOOR`] up: of those no more than 20 LU below their mean, the spread from the 10th
/// to the 95th percentile. Readings count as spread evenly over their bin.
fn loudness_range(count: &[f64], step: f64) -> Option<f64> {
    let total: f64 = count.iter().sum();
    if total == 0.0 {
        return None;
    }
    let start = |i: usize| HIST_FLOOR + i as f64 * step;
    let energy = |lufs: f64| 10f64.powf((lufs + 0.691) / 10.0);
    let mean = count.iter().enumerate().map(|(i, &n)| n * energy(start(i) + step / 2.0)).sum::<f64>() / total;
    let gate = -0.691 + 10.0 * mean.log10() - 20.0;
    // What of each bin lies above the gate.
    let kept: Vec<f64> = count.iter().enumerate().map(|(i, &n)| n * ((start(i) + step - gate) / step).clamp(0.0, 1.0)).collect();
    let n: f64 = kept.iter().sum();
    if n < 2.0 {
        return None;
    }
    let at = |fraction: f64| {
        let want = fraction * n;
        let mut seen = 0.0;
        for (i, &k) in kept.iter().enumerate() {
            if k > 0.0 && seen + k >= want {
                // The part of the bin above the gate, and how far into it the percentile is.
                let from = start(i).max(gate);
                return from + (start(i) + step - from) * ((want - seen) / k);
            }
            seen += k;
        }
        start(kept.len())
    };
    Some((at(0.95) - at(0.10)).max(0.0))
}

/// The loudness of several tracks as one programme (an album): gated over all their blocks
/// together, with the highest of their peaks. None for no tracks, or all of them silent.
pub fn album<'a>(tracks: impl IntoIterator<Item = &'a Measured>) -> Option<Loudness> {
    // A bin stands for its middle…
    let middle = |i: usize| 10f64.powf((HIST_FLOOR + (i as f64 + 0.5) * HIST_STEP + 0.691) / 10.0);
    let mut count = [0f64; HIST_BINS];
    let mut sum = [0f64; HIST_BINS];
    let mut peak = 0f64;
    for m in tracks {
        peak = peak.max(m.peak);
        let bins = || m.hist.iter().enumerate().map(|(i, &n)| (m.hist_from as usize + i, n as f64)).filter(|(i, _)| *i < HIST_BINS);
        let (approx, blocks) = bins().fold((0.0, 0.0), |(e, c), (i, n)| (e + middle(i) * n, c + n));
        if blocks == 0.0 || approx <= 0.0 {
            continue;
        }
        // …scaled so the track's blocks add up to the energy it really had (a steady signal
        // sits in one bin, anywhere in its 1 LU).
        let scale = m.energy * blocks / approx;
        for (i, n) in bins() {
            count[i] += n;
            sum[i] += middle(i) * scale * n;
        }
    }
    let lufs = gated(&count, &sum)?;
    Some(Loudness { gain_db: TARGET_LUFS - lufs, peak: Some(peak) })
}

/// BS.1770 gating over blocks already above the absolute gate, binned by loudness (`count`
/// blocks and their summed energy per bin): the mean of the blocks no more than 10 LU below
/// the mean of all of them. The gate falls on a bin's edge, which moves the result by a few
/// hundredths of a dB at most.
fn gated(count: &[f64; HIST_BINS], sum: &[f64; HIST_BINS]) -> Option<f64> {
    let blocks: f64 = count.iter().sum();
    if blocks == 0.0 {
        return None;
    }
    let gate = sum.iter().sum::<f64>() / blocks * 0.1; // −10 LU
    let (mut s, mut n) = (0.0, 0.0);
    for i in 0..HIST_BINS {
        if count[i] > 0.0 && sum[i] / count[i] >= gate {
            s += sum[i];
            n += count[i];
        }
    }
    (n > 0.0).then(|| -0.691 + 10.0 * (s / n).log10())
}

#[derive(Debug, Clone, Copy)]
struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
}

/// The K-weighting of BS.1770 for any sample rate: a high shelf for the head's effect, then
/// a high-pass. (The standard prints the coefficients for 48 kHz; these are the same analogue
/// filters, redone for `fs`.)
fn k_weighting(fs: f64) -> [Biquad; 2] {
    let shelf = {
        let (f0, g, q) = (1681.974450955533, 3.999843853973347, 0.7071752369554196);
        let k = (PI * f0 / fs).tan();
        let vh = 10f64.powf(g / 20.0);
        let vb = vh.powf(0.4996667741545416);
        let a0 = 1.0 + k / q + k * k;
        Biquad {
            b: [(vh + vb * k / q + k * k) / a0, 2.0 * (k * k - vh) / a0, (vh - vb * k / q + k * k) / a0],
            a: [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0],
        }
    };
    let highpass = {
        let (f0, q) = (38.13547087602444, 0.5003270373238773);
        let k = (PI * f0 / fs).tan();
        let a0 = 1.0 + k / q + k * k;
        Biquad { b: [1.0, -2.0, 1.0], a: [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0] }
    };
    [shelf, highpass]
}

/// Taps per phase of the true-peak interpolator.
const TP_TAPS: usize = 32;

/// The interpolator for true peak: the values between the samples, `over`− 1 of them per
/// sample (4× oversampling up to 48 kHz, 2× up to 96 kHz — above that the samples themselves
/// are close enough, as BS.1770 says).
struct Interpolator {
    /// `over − 1` phases of TP_TAPS coefficients (phase 0 is the sample itself).
    phases: Vec<[f64; TP_TAPS]>,
    /// The most any phase can amplify: Σ|h|.
    reach: f64,
}

impl Interpolator {
    fn new(fs: u32) -> Self {
        let over = if fs < 60_000 { 4 } else if fs < 120_000 { 2 } else { 1 };
        let mut phases = Vec::new();
        for p in 1..over {
            let frac = p as f64 / over as f64;
            let mut h = [0.0; TP_TAPS];
            for (k, c) in h.iter_mut().enumerate() {
                // The point sits `frac` after the sample TP_TAPS/2 − 1 back from the newest.
                let x = k as f64 - (TP_TAPS / 2 - 1) as f64 - frac;
                let sinc = if x.abs() < 1e-12 { 1.0 } else { (PI * x).sin() / (PI * x) };
                // Hann window over the taps' span.
                let w = 0.5 + 0.5 * (PI * x / (TP_TAPS as f64 / 2.0)).cos();
                *c = sinc * w.max(0.0);
            }
            // Unity gain at DC, so a constant comes out as itself.
            let sum: f64 = h.iter().sum();
            h.iter_mut().for_each(|c| *c /= sum);
            phases.push(h);
        }
        let reach = phases.iter().map(|h| h.iter().map(|c| c.abs()).sum::<f64>()).fold(1.0, f64::max);
        Self { phases, reach }
    }
}

/// One channel's state.
struct Channel {
    weight: f64,
    z: [[f64; 2]; 2],
    /// The last TP_TAPS samples, oldest first.
    recent: [f64; TP_TAPS],
    /// Windows still to interpolate after a loud sample came in.
    hot: usize,
}

/// Measures a stream of interleaved frames: integrated loudness (BS.1770-4 / EBU R128) and
/// true peak.
pub struct Meter {
    channels: Vec<Channel>,
    filters: [Biquad; 2],
    interp: Interpolator,
    /// Frames per 100 ms step.
    step: usize,
    in_step: usize,
    /// Weighted sum of squares in the step being filled.
    acc: f64,
    /// Mean square of the last four steps (a 400 ms block ends at each step).
    steps: [f64; 4],
    filled: usize,
    hist: [u32; HIST_BINS],
    /// Exact energies behind `hist`, for this track's own figure.
    sum: [f64; HIST_BINS],
    peak: f64,
    /// Mean square of the last thirty steps: a short-term (3 s) reading ends at each step.
    short: [f64; ST_STEPS],
    /// Short-term readings per [`ST_FINE`] LU, from [`HIST_FLOOR`] up.
    st: Vec<u32>,
    dr: DrMeter,
}

/// Steps (100 ms) in a short-term loudness window.
const ST_STEPS: usize = 30;
/// Width of the bins a track's own loudness range is read from, in LU.
const ST_FINE: f64 = 0.1;

/// The "DR" dynamic range figure (TT Dynamic Range Meter, the one the dynamic range
/// databases collect). Per channel, over blocks of three seconds: how far the second-highest
/// peak stands above the RMS of the loudest fifth of the blocks. The track's figure is the
/// mean over its channels. RMS is taken 3 dB up, so a full-scale sine reads 0.
struct DrMeter {
    /// Frames per block.
    block: usize,
    in_block: usize,
    /// Per channel: sum of squares and peak of the block being filled.
    acc: Vec<(f64, f64)>,
    /// Per channel: (mean square × 2, peak) of each finished block.
    blocks: Vec<Vec<(f64, f64)>>,
}

impl DrMeter {
    fn new(sample_rate: u32, channels: usize) -> Self {
        Self { block: (sample_rate as usize * 3).max(1), in_block: 0, acc: vec![(0.0, 0.0); channels], blocks: vec![Vec::new(); channels] }
    }

    #[inline(always)]
    fn sample(&mut self, channel: usize, x: f64) {
        let a = &mut self.acc[channel];
        a.0 += x * x;
        a.1 = a.1.max(x.abs());
    }

    #[inline(always)]
    fn end_frame(&mut self) {
        self.in_block += 1;
        if self.in_block == self.block {
            self.end_block();
        }
    }

    fn end_block(&mut self) {
        if self.in_block == 0 {
            return;
        }
        let n = self.in_block as f64;
        for (a, blocks) in self.acc.iter_mut().zip(&mut self.blocks) {
            blocks.push((2.0 * a.0 / n, a.1));
            *a = (0.0, 0.0);
        }
        self.in_block = 0;
    }

    /// `counted`: the channels that take part (not the LFE).
    fn finish(mut self, counted: impl Fn(usize) -> bool) -> Option<f64> {
        // What is left of the last block counts as one.
        self.end_block();
        let mut sum = 0.0;
        let mut channels = 0;
        for (c, blocks) in self.blocks.iter_mut().enumerate() {
            if !counted(c) || blocks.is_empty() {
                continue;
            }
            let second_peak = {
                let mut peaks: Vec<f64> = blocks.iter().map(|b| b.1).collect();
                peaks.sort_by(|a, b| b.total_cmp(a));
                peaks[1.min(peaks.len() - 1)]
            };
            blocks.sort_by(|a, b| b.0.total_cmp(&a.0));
            let top = ((blocks.len() as f64 * 0.2) as usize).max(1);
            let rms = (blocks[..top].iter().map(|b| b.0).sum::<f64>() / top as f64).sqrt();
            // Silence (or one lone click in it) has no dynamics to speak of.
            if rms < 1e-6 || second_peak <= 0.0 {
                continue;
            }
            sum += 20.0 * (second_peak / rms).log10();
            channels += 1;
        }
        (channels > 0).then(|| sum / channels as f64)
    }
}

impl Meter {
    /// `layout`: what each channel is, for the weights of BS.1770 (surrounds count 1.5 dB
    /// more, the LFE not at all). Channels beyond it count as ordinary ones.
    pub fn new(sample_rate: u32, channels: usize, layout: &[Speaker]) -> Self {
        let weight = |c: usize| match layout.get(c) {
            Some(Speaker::Lfe) => 0.0,
            Some(Speaker::BackLeft | Speaker::BackRight | Speaker::SideLeft | Speaker::SideRight) => 1.41,
            _ => 1.0,
        };
        Self {
            channels: (0..channels.max(1)).map(|c| Channel { weight: weight(c), z: [[0.0; 2]; 2], recent: [0.0; TP_TAPS], hot: 0 }).collect(),
            filters: k_weighting(sample_rate as f64),
            interp: Interpolator::new(sample_rate),
            step: (sample_rate as usize / 10).max(1),
            in_step: 0,
            acc: 0.0,
            steps: [0.0; 4],
            filled: 0,
            hist: [0; HIST_BINS],
            sum: [0.0; HIST_BINS],
            peak: 0.0,
            short: [0.0; ST_STEPS],
            st: vec![0; (HIST_BINS as f64 * HIST_STEP / ST_FINE) as usize],
            dr: DrMeter::new(sample_rate, channels.max(1)),
        }
    }

    /// Add interleaved frames.
    pub fn push(&mut self, samples: &[f32]) {
        let n = self.channels.len();
        for frame in samples.chunks_exact(n) {
            for (c, (ch, &s)) in self.channels.iter_mut().zip(frame).enumerate() {
                let x = s as f64;
                self.dr.sample(c, x);
                // True peak: the sample, and — only around samples loud enough to matter —
                // the points between.
                let a = x.abs();
                if a > self.peak {
                    self.peak = a;
                }
                ch.recent.copy_within(1.., 0);
                ch.recent[TP_TAPS - 1] = x;
                if a * self.interp.reach > self.peak {
                    ch.hot = TP_TAPS;
                }
                if ch.hot > 0 {
                    ch.hot -= 1;
                    for h in &self.interp.phases {
                        let v: f64 = h.iter().zip(&ch.recent).map(|(c, s)| c * s).sum();
                        if v.abs() > self.peak {
                            self.peak = v.abs();
                        }
                    }
                }
                // Loudness: K-weighted mean square.
                if ch.weight > 0.0 {
                    let mut y = x;
                    for (f, z) in self.filters.iter().zip(ch.z.iter_mut()) {
                        // Transposed direct form II.
                        let out = f.b[0] * y + z[0];
                        z[0] = f.b[1] * y - f.a[0] * out + z[1];
                        z[1] = f.b[2] * y - f.a[1] * out;
                        y = out;
                    }
                    self.acc += ch.weight * y * y;
                }
            }
            self.dr.end_frame();
            self.in_step += 1;
            if self.in_step == self.step {
                self.end_step();
            }
        }
    }

    fn end_step(&mut self) {
        self.steps.copy_within(1.., 0);
        self.steps[3] = self.acc / self.step as f64;
        self.short.copy_within(1.., 0);
        self.short[ST_STEPS - 1] = self.steps[3];
        self.acc = 0.0;
        self.in_step = 0;
        self.filled += 1;
        if self.filled >= ST_STEPS {
            let energy = self.short.iter().sum::<f64>() / ST_STEPS as f64;
            let lufs = if energy > 0.0 { -0.691 + 10.0 * energy.log10() } else { f64::NEG_INFINITY };
            if lufs >= HIST_FLOOR {
                let bin = (((lufs - HIST_FLOOR) / ST_FINE) as usize).min(self.st.len() - 1);
                self.st[bin] += 1;
            }
        }
        if self.filled < 4 {
            return;
        }
        let energy = self.steps.iter().sum::<f64>() / 4.0;
        if energy <= 0.0 {
            return;
        }
        let lufs = -0.691 + 10.0 * energy.log10();
        if lufs < HIST_FLOOR {
            return;
        }
        let bin = (((lufs - HIST_FLOOR) / HIST_STEP) as usize).min(HIST_BINS - 1);
        self.hist[bin] += 1;
        self.sum[bin] += energy;
    }

    pub fn finish(self) -> Measured {
        let count = self.hist.map(|n| n as f64);
        let blocks: f64 = count.iter().sum();
        let from = self.hist.iter().position(|&n| n > 0).unwrap_or(0);
        let to = self.hist.iter().rposition(|&n| n > 0).map_or(from, |i| i + 1);
        // The short-term readings: read finely for this track, kept per 1 LU for its album.
        let fine: Vec<f64> = self.st.iter().map(|&n| n as f64).collect();
        let per_lu = (HIST_STEP / ST_FINE) as usize;
        let coarse: Vec<u32> = self.st.chunks(per_lu).map(|c| c.iter().sum()).collect();
        let st_from = coarse.iter().position(|&n| n > 0).unwrap_or(0);
        let st_to = coarse.iter().rposition(|&n| n > 0).map_or(st_from, |i| i + 1);
        let weights: Vec<f64> = self.channels.iter().map(|c| c.weight).collect();
        Measured {
            lufs: gated(&count, &self.sum),
            peak: self.peak,
            energy: if blocks > 0.0 { self.sum.iter().sum::<f64>() / blocks } else { 0.0 },
            hist_from: from as u16,
            hist: self.hist[from..to].to_vec(),
            dr: self.dr.finish(|c| weights[c] > 0.0),
            lra: loudness_range(&fine, ST_FINE),
            st_from: st_from as u16,
            st: coarse[st_from..st_to].to_vec(),
        }
    }
}

/// Measure a track that is decoded whole and held in memory from its first frame (most
/// tracks, a moment after they are opened). None otherwise: a long file kept in a window, a
/// stream still arriving, DSD on its way out as DoP.
pub fn measure_loaded(track: &Track) -> Option<Measured> {
    if !track.is_complete() || track.is_dop() || track.held_from() != 0 || track.error().is_some() {
        return None;
    }
    let mut meter = Meter::new(track.sample_rate, track.channels, &track.layout);
    let mut pos = 0;
    loop {
        let s = track.slice(pos, 1 << 16);
        if s.is_empty() {
            break;
        }
        meter.push(s);
        pos += s.len() / track.channels;
    }
    // A window that moved on while we read: what we saw wasn't the whole track.
    (pos == track.ready_frames() && track.held_from() == 0).then(|| meter.finish())
}

/// Measure a file (or CUE track) of the library by decoding all of it, off the playback path.
pub fn analyze(path: &std::path::Path) -> anyhow::Result<Measured> {
    let track = crate::open_file(path, crate::dsd::DsdMode::Pcm)?;
    let mut meter = Meter::new(track.sample_rate, track.channels, &track.layout);
    crate::quality::read_all(&track, |s| meter.push(s))?;
    Ok(meter.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: u32, secs: f64, hz: f64, amp: f64, channels: usize) -> Vec<f32> {
        let n = (rate as f64 * secs) as usize;
        (0..n).flat_map(|i| std::iter::repeat_n((amp * (2.0 * PI * hz * i as f64 / rate as f64).sin()) as f32, channels)).collect()
    }

    fn measure(rate: u32, channels: usize, samples: &[f32]) -> Measured {
        let mut m = Meter::new(rate, channels, &crate::channels::default_layout(channels));
        // In uneven pieces, as a decoder delivers them.
        for piece in samples.chunks(channels * 1777) {
            m.push(piece);
        }
        m.finish()
    }

    /// EBU Tech 3341, case 1 and 2: a stereo 1 kHz sine at −23 dBFS reads −23.0 LUFS, at
    /// −33 dBFS −33.0 — at any sample rate.
    #[test]
    fn a_reference_sine_reads_its_level() {
        for rate in [44_100, 48_000, 88_200, 96_000, 192_000] {
            for db in [-23.0, -33.0] {
                let amp = 10f64.powf(db / 20.0);
                let m = measure(rate, 2, &sine(rate, 20.0, 1000.0, amp, 2));
                let lufs = m.lufs.unwrap();
                assert!((lufs - db).abs() < 0.1, "{rate} Hz, {db} dBFS: {lufs:.2} LUFS");
            }
        }
    }

    /// EBU Tech 3341, case 3: −36, −23, −36 dBFS in turn. The quiet parts fall under the
    /// relative gate, so the result is the loud part's −23.0 LUFS.
    #[test]
    fn quiet_passages_are_gated_out() {
        let rate = 48_000;
        let part = |db: f64, secs: f64| sine(rate, secs, 1000.0, 10f64.powf(db / 20.0), 2);
        let mut s = part(-36.0, 10.0);
        s.extend(part(-23.0, 60.0));
        s.extend(part(-36.0, 10.0));
        let lufs = measure(rate, 2, &s).lufs.unwrap();
        assert!((lufs + 23.0).abs() < 0.1, "{lufs:.2}");
    }

    #[test]
    fn silence_has_no_loudness() {
        let m = measure(44_100, 2, &vec![0.0; 44_100 * 2 * 3]);
        assert_eq!(m.lufs, None);
        assert_eq!(m.loudness(), None);
        assert!(m.hist.is_empty());
    }

    /// A sine at a quarter of the sample rate, sampled 45° off its crests: every sample is
    /// 3 dB below the real peak. (The classic case from BS.1770's annex.)
    #[test]
    fn true_peak_finds_what_lies_between_the_samples() {
        let rate = 44_100u32;
        let s: Vec<f32> = (0..rate * 2).map(|i| (0.9 * (2.0 * PI * (i as f64 / 4.0 + 0.125)).sin()) as f32).collect();
        let sample_peak = s.iter().fold(0f32, |m, v| m.max(v.abs())) as f64;
        assert!((sample_peak - 0.9 * std::f64::consts::FRAC_1_SQRT_2).abs() < 0.001);
        let m = measure(rate, 1, &s);
        assert!((m.peak - 0.9).abs() < 0.02, "true peak {:.4}", m.peak);
    }

    /// Skipping the quiet samples must not change the answer: the same audio with a loud
    /// stretch in the middle gives that stretch's true peak.
    #[test]
    fn true_peak_of_a_loud_stretch_among_quiet_ones() {
        let rate = 48_000u32;
        let quiet = |i: u32| 0.05 * (2.0 * PI * 440.0 * i as f64 / rate as f64).sin();
        let loud = |i: u32| 0.8 * (2.0 * PI * (i as f64 / 4.0 + 0.125)).sin();
        let s: Vec<f32> = (0..rate * 3).map(|i| if (rate..rate + 4800).contains(&i) { loud(i) } else { quiet(i) } as f32).collect();
        let m = measure(rate, 1, &s);
        assert!((m.peak - 0.8).abs() < 0.03, "true peak {:.4}", m.peak);
    }

    #[test]
    fn an_album_is_gated_as_one_programme() {
        let rate = 48_000;
        let track = |db: f64| measure(rate, 2, &sine(rate, 30.0, 1000.0, 10f64.powf(db / 20.0), 2));
        // Two tracks 6 dB apart: the album's loudness is their energy mean (both above the
        // gate), 0.97 dB under the louder one.
        let (a, b) = (track(-20.0), track(-26.0));
        let album = album([&a, &b]).unwrap();
        let lufs = TARGET_LUFS - album.gain_db;
        let expect = 10.0 * ((10f64.powf(-2.0) + 10f64.powf(-2.6)) / 2.0).log10();
        assert!((lufs - expect).abs() < 0.15, "{lufs:.2} vs {expect:.2}");
        assert_eq!(album.peak, Some(a.peak.max(b.peak)));
        // A far quieter track (an interlude) falls under the album's gate and changes nothing.
        let c = track(-45.0);
        let with = super::album([&a, &c]).unwrap();
        assert!((TARGET_LUFS - with.gain_db + 20.0).abs() < 0.15);
        // One track: the same as its own figure, within the histogram's grain.
        let one = super::album([&a]).unwrap();
        assert!((one.gain_db - a.loudness().unwrap().gain_db).abs() < 0.1);
    }

    /// EBU Tech 3342, cases 1 to 4: steady tones at two (or three) levels read the distance
    /// between them, within 1 LU; what lies more than 20 LU under the mean is left out.
    #[test]
    fn loudness_range_reads_the_reference_cases() {
        let rate = 48_000;
        let part = |db: f64, secs: f64| sine(rate, secs, 1000.0, 10f64.powf(db / 20.0), 2);
        let lra = |parts: &[(f64, f64)]| {
            let s: Vec<f32> = parts.iter().flat_map(|&(db, secs)| part(db, secs)).collect();
            measure(rate, 2, &s).lra.unwrap()
        };
        for (parts, expect) in [
            (&[(-20.0, 20.0), (-30.0, 20.0)][..], 10.0),
            (&[(-20.0, 20.0), (-15.0, 20.0)][..], 5.0),
            (&[(-40.0, 20.0), (-20.0, 20.0)][..], 20.0),
            (&[(-50.0, 20.0), (-35.0, 20.0), (-20.0, 20.0), (-35.0, 20.0), (-50.0, 20.0)][..], 15.0),
        ] {
            let got = lra(parts);
            assert!((got - expect).abs() < 1.0, "{parts:?}: {got:.2} LU, expected {expect}");
        }
        // A steady tone has no range; three seconds are the least it takes to say anything.
        assert!(measure(rate, 2, &part(-20.0, 30.0)).lra.unwrap() < 0.2);
        assert_eq!(measure(rate, 2, &part(-20.0, 2.0)).lra, None);
    }

    /// An album's range is that of its tracks as one programme: a loud track and a quiet one,
    /// each steady in itself, make an album with the range between them.
    #[test]
    fn an_album_has_the_loudness_range_of_its_tracks_together() {
        let rate = 44_100;
        let track = |db: f64| measure(rate, 2, &sine(rate, 30.0, 1000.0, 10f64.powf(db / 20.0), 2));
        let (loud, quiet) = (track(-12.0), track(-24.0));
        assert!(loud.lra.unwrap() < 0.2 && quiet.lra.unwrap() < 0.2);
        let together = album_lra([&loud, &quiet]).unwrap();
        assert!((together - 12.0).abs() < 1.0, "{together:.2}");
        assert_eq!(album_lra([&measure(rate, 2, &vec![0.0; 44_100 * 2 * 5])]), None);
        // Results from before the range was measured have nothing to add.
        let old = Measured { st: Vec::new(), st_from: 0, lra: None, dr: None, ..loud.clone() };
        assert_eq!(album_lra([&old]), None);
    }

    /// The DR figure: a steady sine reads 0 (its RMS, taken 3 dB up, is its peak) at any
    /// level; music with peaks above its body reads how far above.
    #[test]
    fn dynamic_range_is_peak_over_the_loudest_fifth() {
        let rate = 44_100;
        for amp in [1.0, 0.1] {
            let m = measure(rate, 2, &sine(rate, 30.0, 1000.0, amp, 2));
            assert!(m.dr.unwrap().abs() < 0.05, "{:?}", m.dr);
            assert_eq!(m.dr_rounded(), Some(0));
        }
        // Twenty blocks of three seconds: two at full scale, eighteen at a tenth of it. The
        // loudest fifth is four blocks, two of each: RMS² = (1 + 1 + 0.01 + 0.01) / 4.
        let mut s = sine(rate, 6.0, 1000.0, 1.0, 2);
        s.extend(sine(rate, 54.0, 1000.0, 0.1, 2));
        let m = measure(rate, 2, &s);
        let expect = -10.0 * (2.02f64 / 4.0).log10();
        assert!((m.dr.unwrap() - expect).abs() < 0.05, "{:?} vs {expect:.2}", m.dr);
        // One loud block alone is not the peak the figure goes by: the second-highest is.
        let mut s = sine(rate, 3.0, 1000.0, 1.0, 2);
        s.extend(sine(rate, 57.0, 1000.0, 0.1, 2));
        assert!(measure(rate, 2, &s).dr.unwrap() < 0.0);
        // Silence has none.
        assert_eq!(measure(rate, 2, &vec![0.0; 44_100 * 2 * 4]).dr, None);
        // Channels are read one by one and averaged: a silent one is left out.
        let one_sided: Vec<f32> = sine(rate, 30.0, 1000.0, 0.5, 1).into_iter().flat_map(|v| [v, 0.0]).collect();
        assert!(measure(rate, 2, &one_sided).dr.unwrap().abs() < 0.05);
    }

    #[test]
    fn an_album_s_dr_is_the_mean_of_its_tracks_rounded() {
        let with = |dr: Option<f64>| Measured { lufs: None, peak: 0.0, energy: 0.0, hist_from: 0, hist: Vec::new(), dr, lra: None, st_from: 0, st: Vec::new() };
        assert_eq!(album_dr([&with(Some(7.6)), &with(Some(9.4)), &with(Some(12.5))]), Some(10));
        assert_eq!(album_dr([&with(Some(5.4)), &with(None)]), Some(5));
        assert_eq!(album_dr([&with(None)]), None);
        assert_eq!(album_dr([]), None);
    }

    /// What was stored before the dynamics were measured still reads, as "not measured".
    #[test]
    fn older_results_still_read() {
        let old = r#"{"lufs":-9.5,"peak":0.98,"energy":0.11,"hist_from":50,"hist":[3,9,40]}"#;
        let m: Measured = serde_json::from_str(old).unwrap();
        assert_eq!((m.lufs, m.dr, m.lra, m.st.len()), (Some(-9.5), None, None, 0));
        let again: Measured = serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
        assert_eq!(again, m);
    }

    #[test]
    fn tags_are_read_as_scanners_write_them() {
        assert_eq!(parse_gain("-6.48 dB"), Some(-6.48));
        assert_eq!(parse_gain("+2.1dB"), Some(2.1));
        assert_eq!(parse_gain(" -7,30 db "), Some(-7.3));
        assert_eq!(parse_gain("loud"), None);
        assert_eq!(parse_gain("-900 dB"), None);
        assert_eq!(parse_peak("0.988553"), Some(0.988553));
        assert_eq!(parse_peak("0"), None);
        // R128: 0 means "at −23 LUFS", which is 5 dB under our target.
        assert_eq!(parse_r128("0"), Some(5.0));
        assert_eq!(parse_r128("-1280"), Some(0.0));
    }

    const TRACK: Loudness = Loudness { gain_db: -8.0, peak: Some(0.99) };
    const ALBUM: Loudness = Loudness { gain_db: -6.0, peak: Some(1.0) };

    #[test]
    fn the_mode_picks_track_or_album_and_falls_back_to_the_other() {
        let on = |mode| LevelSettings { mode, headroom: false };
        let both = Known { track: Some(TRACK), album: Some(ALBUM) };
        let l = resolve(&on(LevelMode::Track), false, &both).unwrap();
        assert!((l.gain_db() + 8.0).abs() < 1e-9 && l.basis == Basis::Track && !l.limited);
        let l = resolve(&on(LevelMode::Album), false, &both).unwrap();
        assert!((l.gain_db() + 6.0).abs() < 1e-9 && l.basis == Basis::Album);
        let l = resolve(&on(LevelMode::Album), false, &Known { track: Some(TRACK), album: None }).unwrap();
        assert_eq!(l.basis, Basis::Track);
        let l = resolve(&on(LevelMode::Track), false, &Known::default()).unwrap();
        assert_eq!((l.gain, l.basis), (1.0, Basis::Unknown));
        assert_eq!(resolve(&LevelSettings::default(), true, &both), None);
    }

    #[test]
    fn a_boost_stops_where_the_peak_would_clip() {
        let quiet = Known { track: Some(Loudness { gain_db: 9.0, peak: Some(0.5) }), album: None };
        let l = resolve(&LevelSettings { mode: LevelMode::Track, headroom: false }, false, &quiet).unwrap();
        assert!(l.limited);
        assert!((l.gain * 0.5 - CEILING).abs() < 1e-9);
        // No peak in the tags: a boost is held to what a full-scale track could take.
        let blind = Known { track: Some(Loudness { gain_db: 4.0, peak: None }), album: None };
        let l = resolve(&LevelSettings { mode: LevelMode::Track, headroom: false }, false, &blind).unwrap();
        assert!(l.limited && l.gain <= CEILING);
    }

    #[test]
    fn headroom_lowers_only_what_would_overshoot() {
        let s = LevelSettings { mode: LevelMode::Off, headroom: true };
        let hot = Known { track: Some(Loudness { gain_db: -9.0, peak: Some(1.12) }), album: None };
        let l = resolve(&s, true, &hot).unwrap();
        assert_eq!(l.basis, Basis::Peak);
        assert!((l.gain * 1.12 - CEILING).abs() < 1e-9);
        // Nothing else is processing: nothing rebuilds the waveform, the samples go out as
        // they are.
        assert_eq!(resolve(&s, false, &hot), None);
        // A track with room to spare is left alone.
        let calm = Known { track: Some(Loudness { gain_db: -3.0, peak: Some(0.8) }), album: None };
        assert_eq!(resolve(&s, true, &calm), None);
        // The album's peak, when known, for every track of it.
        let in_album = Known { track: calm.track, album: Some(Loudness { gain_db: -5.0, peak: Some(1.05) }) };
        assert!((resolve(&s, true, &in_album).unwrap().gain * 1.05 - CEILING).abs() < 1e-9);
    }
}
