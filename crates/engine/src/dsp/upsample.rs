//! Exact ×2 upsampling with very long filters, f64 throughout.
//!
//! All the steepness of the upsampling chain lives in this one stage: it doubles the rate
//! (fs → 2·fs) with a long FIR, so the band from the input's Nyquist fN upwards — every image
//! of the input — is rejected right where it starts. Any further rate increase is left to the
//! short polyphase resampler ([`super::resample`]), whose job is then easy: the signal it gets
//! is already band-limited to half its own Nyquist.
//!
//! A ×2 FIR splits into two polyphase branches of taps/2 coefficients — the even taps make the
//! even output samples, the odd taps the odd ones — and both run on the same input at the
//! input rate. Each branch is a uniformly partitioned overlap-save convolution with real FFTs:
//! the input of every block is transformed once per channel, its spectrum goes into a
//! frequency-domain delay line shared by both branches, and each branch's output block is the
//! sum of P spectral products. Cost per input frame and channel is about 2·P complex
//! multiply-adds plus three FFTs of 2B points, whatever the length of the filter.
//!
//! Filters (all frequencies relative to fN, so a design serves every rate):
//! - Linear: Kaiser-windowed sinc designed for 160 dB (153 dB measured), stop band from
//!   exactly fN, pass band up to fN − Δ with Δ as narrow as the length allows: 14 Hz at
//!   88.2 kHz for 65,536 taps, 0.9 Hz for 2^20.
//!   Symmetric; its delay (taps/2 output samples) is skipped at the start and flushed at the
//!   end, so an impulse at input frame n peaks at output frame 2n.
//! - Minimum: the linear design's magnitude with minimum phase (homomorphic method, as in
//!   [`super::fir`]), same stop band: no pre-ringing and no delay to skip, at the cost of
//!   phase shift near fN. Up to [`MAX_MINIMUM_TAPS`].
//! - Apodizing: short and minimum phase, flat to 0.907·fN, ≥150 dB from 0.955·fN: it cuts
//!   away the ringing a steep mastering filter left just below fN and puts a short post-only
//!   ring in its place.
//!
//! Work happens a block at a time (taps/64 input frames, at least 512): input is collected
//! until a block is full, and output comes out in bursts of a block's worth of frame pairs.
//!
//! Memory, roughly: the designed filter holds 24 bytes per tap (the taps themselves, and both
//! branches' partition spectra), each [`Doubler`] 8 bytes per tap and channel (its delay line).

use std::f64::consts::PI;
use std::sync::{Arc, Mutex, OnceLock};

use realfft::num_complex::Complex;
use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};

type C = Complex<f64>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FilterKind {
    Linear,
    Minimum,
    Apodizing,
}

/// `taps`: total filter length counted at the doubled rate (so each of the two polyphase
/// branches has taps/2 coefficients). A power of two from [`MIN_TAPS`] to [`MAX_TAPS`] (other
/// values are rounded up into that range); Minimum stops at [`MAX_MINIMUM_TAPS`]. Ignored for
/// Apodizing, whose length follows from its fixed transition band. [`FilterSpec::effective`]
/// says what will actually be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FilterSpec {
    pub kind: FilterKind,
    pub taps: usize,
}

pub const MIN_TAPS: usize = 4096;
pub const MAX_TAPS: usize = 1 << 24;
/// Longest minimum-phase design: the homomorphic method works on a spectrum
/// [`MIN_PHASE_OVERSAMPLE`]× longer than the filter, which at 2^24 taps would need several GB.
/// Longer requests get this length.
pub const MAX_MINIMUM_TAPS: usize = 1 << 20;
/// Length of the apodizing filter (doubled rate).
pub const APODIZING_TAPS: usize = 1024;

impl FilterSpec {
    /// The spec as built: length rounded into range, Apodizing's fixed length filled in.
    pub fn effective(self) -> Self {
        let taps = match self.kind {
            FilterKind::Apodizing => APODIZING_TAPS,
            FilterKind::Linear => self.taps.clamp(MIN_TAPS, MAX_TAPS).next_power_of_two(),
            FilterKind::Minimum => self.taps.clamp(MIN_TAPS, MAX_MINIMUM_TAPS).next_power_of_two(),
        };
        Self { kind: self.kind, taps }
    }

    /// The next cheaper setting, for when the processing can't keep up: the sharp filters step
    /// down 2^24 → 2^20 → 2^16 taps, and below that (and from Apodizing, already short) None:
    /// the standard resampler.
    pub fn shorter(self) -> Option<FilterSpec> {
        let s = self.effective();
        let taps = match s.kind {
            FilterKind::Apodizing => return None,
            FilterKind::Linear | FilterKind::Minimum if s.taps > 1 << 20 => 1 << 20,
            FilterKind::Linear | FilterKind::Minimum if s.taps > 1 << 16 => 1 << 16,
            _ => return None,
        };
        Some(Self { kind: s.kind, taps })
    }
}

/// Stop-band attenuation the Kaiser designs aim at (dB). Kaiser's length formula is an
/// approximation; the margin over the 150 dB we want covers it.
const SHARP_ATTEN_DB: f64 = 160.0;
const APODIZING_ATTEN_DB: f64 = 165.0;
/// Apodizing pass and stop band edges, as fractions of fN (20 kHz and 21.06 kHz at 44.1k).
pub const APODIZING_PASS: f64 = 0.907;
pub const APODIZING_STOP: f64 = 0.955;
/// The minimum-phase design samples |H| this many times finer than the filter length.
pub const MIN_PHASE_OVERSAMPLE: usize = 16;
/// |H| is floored here (dB below the pass band) before its logarithm is taken: the stop
/// band's exact zeros would otherwise be −∞.
const MIN_PHASE_FLOOR_DB: f64 = 200.0;
/// Partitions to aim for: more cut the block (latency, delay-line granularity) but add
/// multiply-adds per sample; fewer make each FFT longer.
const TARGET_PARTS: usize = 32;
const MIN_BLOCK: usize = 512;

/// Zeroth-order modified Bessel function of the first kind (for the Kaiser window).
fn bessel_i0(x: f64) -> f64 {
    let (mut sum, mut term, mut k) = (1.0, 1.0, 1.0);
    while term > sum * 1e-17 {
        term *= (x / (2.0 * k)) * (x / (2.0 * k));
        sum += term;
        k += 1.0;
    }
    sum
}

/// Kaiser β for `a` dB of stop-band attenuation.
fn kaiser_beta(a: f64) -> f64 {
    0.1102 * (a - 8.7)
}

/// Full transition width (cycles per sample) Kaiser's formula gives `a` dB in `n` taps.
fn kaiser_transition(a: f64, n: usize) -> f64 {
    (a - 8.0) / (14.36 * (n - 2) as f64)
}

/// Kaiser-windowed sinc low-pass of `n` taps (even), cut-off `fc` cycles per sample.
/// Symmetric about tap n/2, so its delay is a whole number of samples; tap 0 has no mirror
/// and stays zero.
fn kaiser_lowpass(n: usize, fc: f64, beta: f64) -> Vec<f64> {
    let half = n / 2;
    let i0_beta = bessel_i0(beta);
    let mut h = vec![0.0; n];
    for t in 0..half {
        let x = t as f64;
        // sin(2π·fc·t) with the phase reduced first: t reaches millions of samples.
        let sinc = if t == 0 { 2.0 * fc } else { (2.0 * PI * (fc * x).fract()).sin() / (PI * x) };
        let r = x / half as f64;
        let v = sinc * bessel_i0(beta * (1.0 - r * r).sqrt()) / i0_beta;
        h[half + t] = v;
        h[half - t] = v;
    }
    h
}

/// Minimum-phase filter with the magnitude of `lin` (homomorphic method: the real cepstrum of
/// ln|H| folded onto positive time). |H| is sampled [`MIN_PHASE_OVERSAMPLE`]× finer than the
/// filter length: the cepstrum of a filter this steep decays slowly, and whatever does not fit
/// wraps around and fills the stop band.
fn minimum_phase(lin: &[f64], oversample: usize, floor_db: f64) -> Vec<f64> {
    let n = lin.len();
    let m = n * oversample;
    let mut planner = RealFftPlanner::<f64>::new();
    let (fwd, inv) = (planner.plan_fft_forward(m), planner.plan_fft_inverse(m));
    let mut scratch = vec![C::new(0.0, 0.0); fwd.get_scratch_len().max(inv.get_scratch_len())];
    let mut time = vec![0.0; m];
    time[..n].copy_from_slice(lin);
    let mut spec = fwd.make_output_vec();
    fwd.process_with_scratch(&mut time, &mut spec, &mut scratch).expect("fft length");
    let peak = spec.iter().fold(0.0f64, |m, v| m.max(v.norm()));
    let floor = peak * 10f64.powf(-floor_db / 20.0);
    for v in spec.iter_mut() {
        *v = C::new(v.norm().max(floor).ln(), 0.0);
    }
    inv.process_with_scratch(&mut spec, &mut time, &mut scratch).expect("fft length");
    let s = 1.0 / m as f64;
    time[0] *= s;
    time[1..m / 2].iter_mut().for_each(|v| *v *= 2.0 * s);
    time[m / 2] *= s;
    time[m / 2 + 1..].fill(0.0);
    fwd.process_with_scratch(&mut time, &mut spec, &mut scratch).expect("fft length");
    for v in spec.iter_mut() {
        *v = v.exp();
    }
    spec[0].im = 0.0;
    spec[m / 2].im = 0.0;
    inv.process_with_scratch(&mut spec, &mut time, &mut scratch).expect("fft length");
    time.truncate(n);
    time.iter_mut().for_each(|v| *v *= s);
    time
}

/// Scale each polyphase branch to sum to exactly 1: unity DC gain at every output sample,
/// and an exact zero at the output Nyquist (the image of DC).
fn normalise_branches(h: &mut [f64]) {
    for b in 0..2 {
        let sum: f64 = h.iter().skip(b).step_by(2).sum();
        h.iter_mut().skip(b).step_by(2).for_each(|v| *v /= sum);
    }
}

/// Block size for branches of `len` taps.
fn block_for(len: usize) -> usize {
    (len / TARGET_PARTS).next_power_of_two().max(MIN_BLOCK)
}

/// A designed filter, ready to run: shared between players and channels, immutable.
pub struct DoublerFilter {
    spec: FilterSpec,
    taps: Vec<f64>,
    /// Output samples skipped to keep the music time-aligned (even).
    delay: usize,
    /// Pass and stop band edges, fractions of fN.
    pass: f64,
    stop: f64,
    block: usize,
    parts: usize,
    /// Partition spectra, `parts × (block + 1)` bins per branch, pre-scaled by the inverse
    /// FFT's 1/(2·block).
    even: Vec<C>,
    odd: Vec<C>,
}

impl DoublerFilter {
    fn design(spec: FilterSpec) -> Self {
        let spec = spec.effective();
        let n = spec.taps;
        // Frequencies in cycles per output sample: fN = 0.25.
        let (mut taps, delay, pass, stop) = match spec.kind {
            FilterKind::Linear | FilterKind::Minimum => {
                let delta = kaiser_transition(SHARP_ATTEN_DB, n);
                let lin = kaiser_lowpass(n, 0.25 - delta / 2.0, kaiser_beta(SHARP_ATTEN_DB));
                let pass = 1.0 - delta / 0.25;
                match spec.kind {
                    FilterKind::Linear => (lin, n / 2, pass, 1.0),
                    _ => (minimum_phase(&lin, MIN_PHASE_OVERSAMPLE, MIN_PHASE_FLOOR_DB), 0, pass, 1.0),
                }
            }
            FilterKind::Apodizing => {
                let fc = (APODIZING_PASS + APODIZING_STOP) / 2.0 * 0.25;
                let lin = kaiser_lowpass(n, fc, kaiser_beta(APODIZING_ATTEN_DB));
                (minimum_phase(&lin, MIN_PHASE_OVERSAMPLE, MIN_PHASE_FLOOR_DB), 0, APODIZING_PASS, APODIZING_STOP)
            }
        };
        normalise_branches(&mut taps);

        let len = n / 2;
        let b = block_for(len);
        let parts = len.div_ceil(b);
        let mut planner = RealFftPlanner::<f64>::new();
        let fwd = planner.plan_fft_forward(2 * b);
        let mut scratch = fwd.make_scratch_vec();
        let mut time = vec![0.0; 2 * b];
        let scale = 1.0 / (2 * b) as f64;
        let mut branch = |phase: usize| -> Vec<C> {
            let mut out = vec![C::new(0.0, 0.0); parts * (b + 1)];
            for (p, bins) in out.chunks_exact_mut(b + 1).enumerate() {
                time.fill(0.0);
                for (i, slot) in time[..b].iter_mut().enumerate() {
                    if let Some(&t) = taps.get(2 * (p * b + i) + phase) {
                        *slot = t * scale;
                    }
                }
                fwd.process_with_scratch(&mut time, bins, &mut scratch).expect("fft length");
            }
            out
        };
        let (even, odd) = (branch(0), branch(1));
        Self { spec, taps, delay, pass, stop, block: b, parts, even, odd }
    }

    /// The spec this filter was built for (after [`FilterSpec::effective`]).
    pub fn spec(&self) -> FilterSpec {
        self.spec
    }

    /// Impulse response at the doubled rate. Sums to 2 (each branch to 1): zero-stuffing
    /// halves the level, the filter restores it.
    pub fn taps(&self) -> &[f64] {
        &self.taps
    }

    /// Output samples skipped to keep the music time-aligned (0 for minimum phase).
    pub fn delay(&self) -> usize {
        self.delay
    }

    /// Pass band edge as a fraction of the input Nyquist (fN − Δ for the sharp filters).
    pub fn passband(&self) -> f64 {
        self.pass
    }

    /// Stop band edge as a fraction of the input Nyquist (1 for the sharp filters).
    pub fn stopband(&self) -> f64 {
        self.stop
    }

    /// Input frames per FFT block.
    pub fn block(&self) -> usize {
        self.block
    }

    pub fn partitions(&self) -> usize {
        self.parts
    }

    /// Input frames a new or reset [`Doubler`] takes before it puts anything out: the delay to
    /// skip, rounded up to whole blocks.
    pub fn startup_frames(&self) -> usize {
        (self.delay / 2 / self.block + 1) * self.block
    }

    pub fn memory_bytes(&self) -> usize {
        self.taps.len() * size_of::<f64>() + (self.even.len() + self.odd.len()) * size_of::<C>()
    }
}

type Slot = Arc<OnceLock<Arc<DoublerFilter>>>;

/// The last two designs asked for, most recent last.
static CACHE: Mutex<Vec<(FilterSpec, Slot)>> = Mutex::new(Vec::new());
const CACHE_SIZE: usize = 2;

/// Design (or fetch from a small process-wide cache: the last 2 designs) the filter for
/// `spec`. A design is rate-independent, so the cache key is the spec alone. Long
/// minimum-phase designs take seconds: callers run this off the audio path. Thread-safe; two
/// callers asking for the same spec at once share one design.
pub fn filter(spec: FilterSpec) -> Arc<DoublerFilter> {
    let spec = spec.effective();
    let slot = {
        let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
        let entry = match cache.iter().position(|(s, _)| *s == spec) {
            Some(i) => cache.remove(i),
            None => (spec, Arc::new(OnceLock::new())),
        };
        let slot = entry.1.clone();
        cache.push(entry);
        if cache.len() > CACHE_SIZE {
            cache.remove(0);
        }
        slot
    };
    // Designed outside the lock: other specs need not wait for this one.
    slot.get_or_init(|| Arc::new(DoublerFilter::design(spec))).clone()
}

struct Chan {
    /// Previous and current input block (overlap-save keeps one block of history).
    prev: Vec<f64>,
    cur: Vec<f64>,
    /// Input spectra, `parts × (block + 1)` bins, newest at `head`.
    fdl: Vec<C>,
    /// Each branch's spectral sum, then its output block.
    acc_even: Vec<C>,
    acc_odd: Vec<C>,
    even: Vec<f64>,
    odd: Vec<f64>,
}

/// Streaming ×2 upsampler for interleaved f64 audio.
pub struct Doubler {
    filter: Arc<DoublerFilter>,
    channels: usize,
    fwd: Arc<dyn RealToComplex<f64>>,
    inv: Arc<dyn ComplexToReal<f64>>,
    scratch: Vec<C>,
    time: Vec<f64>,
    chans: Vec<Chan>,
    head: usize,
    /// Frames in the current input block.
    fill: usize,
    /// Real input frames taken since the start / last reset.
    fed: usize,
    /// Output frame pairs computed (including the skipped delay).
    produced: usize,
}

impl Doubler {
    pub fn new(filter: Arc<DoublerFilter>, channels: usize) -> Self {
        assert!(channels > 0);
        let (b, k) = (filter.block, filter.block + 1);
        let mut planner = RealFftPlanner::<f64>::new();
        let (fwd, inv) = (planner.plan_fft_forward(2 * b), planner.plan_fft_inverse(2 * b));
        let scratch = vec![C::new(0.0, 0.0); fwd.get_scratch_len().max(inv.get_scratch_len())];
        let chans = (0..channels)
            .map(|_| Chan {
                prev: vec![0.0; b],
                cur: vec![0.0; b],
                fdl: vec![C::new(0.0, 0.0); filter.parts * k],
                acc_even: vec![C::new(0.0, 0.0); k],
                acc_odd: vec![C::new(0.0, 0.0); k],
                even: vec![0.0; b],
                odd: vec![0.0; b],
            })
            .collect();
        Self {
            channels,
            fwd,
            inv,
            scratch,
            time: vec![0.0; 2 * b],
            chans,
            head: 0,
            fill: 0,
            fed: 0,
            produced: 0,
            filter,
        }
    }

    pub fn filter(&self) -> &Arc<DoublerFilter> {
        &self.filter
    }

    /// Forget all history (a seek). After it the stage behaves like new.
    pub fn reset(&mut self) {
        for c in &mut self.chans {
            c.prev.fill(0.0);
            c.cur.fill(0.0);
            c.fdl.fill(C::new(0.0, 0.0));
        }
        self.head = 0;
        self.fill = 0;
        self.fed = 0;
        self.produced = 0;
    }

    /// Feed interleaved input frames; appends interleaved output (twice the rate) to `out`.
    pub fn process(&mut self, input: &[f64], out: &mut Vec<f64>) {
        let ch = self.channels;
        debug_assert_eq!(input.len() % ch, 0);
        let b = self.filter.block;
        let frames = input.len() / ch;
        let mut i = 0;
        while i < frames {
            let take = (b - self.fill).min(frames - i);
            let run = &input[i * ch..(i + take) * ch];
            for (c, chan) in self.chans.iter_mut().enumerate() {
                for (dst, frame) in chan.cur[self.fill..self.fill + take].iter_mut().zip(run.chunks_exact(ch)) {
                    *dst = frame[c];
                }
            }
            self.fill += take;
            self.fed += take;
            i += take;
            if self.fill == b {
                self.block(out);
            }
        }
    }

    /// End of stream: push the tail out, so that the output has exactly twice as many frames
    /// as the input. Leaves the stage reset, ready for a new stream.
    pub fn flush(&mut self, out: &mut Vec<f64>) {
        let d = self.filter.delay / 2;
        while self.fed > 0 && self.produced < d + self.fed {
            for c in &mut self.chans {
                c.cur[self.fill..].fill(0.0);
            }
            self.block(out);
        }
        self.reset();
    }

    fn block(&mut self, out: &mut Vec<f64>) {
        let Self { filter, fwd, inv, scratch, time, chans, head, .. } = self;
        let (b, parts) = (filter.block, filter.parts);
        let k = b + 1;
        for chan in chans.iter_mut() {
            time[..b].copy_from_slice(&chan.prev);
            time[b..].copy_from_slice(&chan.cur);
            fwd.process_with_scratch(time, &mut chan.fdl[*head * k..(*head + 1) * k], scratch).expect("fft length");
            chan.acc_even.fill(C::new(0.0, 0.0));
            chan.acc_odd.fill(C::new(0.0, 0.0));
        }
        // Σ over partitions j of filter partition j × the input from j blocks ago. Channels
        // take turns on each partition, so it is still in cache for the second one.
        for j in 0..parts {
            let slot = (*head + parts - j) % parts;
            let (he, ho) = (&filter.even[j * k..(j + 1) * k], &filter.odd[j * k..(j + 1) * k]);
            for chan in chans.iter_mut() {
                let x = &chan.fdl[slot * k..(slot + 1) * k];
                for ((((ae, ao), x), he), ho) in chan.acc_even.iter_mut().zip(chan.acc_odd.iter_mut()).zip(x).zip(he).zip(ho) {
                    *ae += x * he;
                    *ao += x * ho;
                }
            }
        }
        for chan in chans.iter_mut() {
            for (acc, dst) in [(&mut chan.acc_even, &mut chan.even), (&mut chan.acc_odd, &mut chan.odd)] {
                // Real signals: DC and Nyquist bins are real (exactly, but say so for the FFT).
                acc[0].im = 0.0;
                acc[b].im = 0.0;
                inv.process_with_scratch(acc, time, scratch).expect("fft length");
                dst.copy_from_slice(&time[b..]);
            }
            std::mem::swap(&mut chan.prev, &mut chan.cur);
        }
        *head = (*head + 1) % parts;
        self.fill = 0;

        // Frame pair n of the filter output is music frame n − d; keep 0 ≤ n − d < fed.
        let d = self.filter.delay / 2;
        let start = d.max(self.produced) - self.produced;
        let end = (d + self.fed).min(self.produced + b).saturating_sub(self.produced);
        if end > start {
            out.reserve((end - start) * 2 * self.channels);
            for i in start..end {
                out.extend(self.chans.iter().map(|c| c.even[i]));
                out.extend(self.chans.iter().map(|c| c.odd[i]));
            }
        }
        self.produced += b;
    }

    pub fn memory_bytes(&self) -> usize {
        let per_chan: usize = self
            .chans
            .iter()
            .map(|c| {
                (c.prev.len() + c.cur.len() + c.even.len() + c.odd.len()) * size_of::<f64>()
                    + (c.fdl.len() + c.acc_even.len() + c.acc_odd.len()) * size_of::<C>()
            })
            .sum();
        per_chan + self.time.len() * size_of::<f64>() + self.scratch.len() * size_of::<C>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(kind: FilterKind, taps: usize) -> FilterSpec {
        FilterSpec { kind, taps }
    }

    /// Repeatable noise in −0.5 … 0.5.
    fn noise(n: usize, seed: u64) -> Vec<f64> {
        let mut x = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        (0..n)
            .map(|_| {
                x = x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
                (x >> 11) as f64 / (1u64 << 53) as f64 - 0.5
            })
            .collect()
    }

    /// Feed `input` in chunks of `chunk` frames, then flush.
    fn run(d: &mut Doubler, input: &[f64], chunk: usize) -> Vec<f64> {
        let mut out = Vec::new();
        for c in input.chunks(chunk * d.channels) {
            d.process(c, &mut out);
        }
        d.flush(&mut out);
        out
    }

    /// The zero-stuffed input convolved with `h` the slow way, `delay` output samples skipped,
    /// interleaved like the Doubler's output.
    fn direct(h: &[f64], delay: usize, input: &[f64], ch: usize) -> Vec<f64> {
        let frames = input.len() / ch;
        let mut out = vec![0.0; 2 * frames * ch];
        for c in 0..ch {
            for m in 0..2 * frames {
                let t = m + delay;
                // y[t] = Σ h[k]·u[t − k], where u[2n] = x[n] and the odd u are zero.
                let mut acc = 0.0;
                let mut k = t % 2;
                while k < h.len() && k <= t {
                    let n = (t - k) / 2;
                    if n < frames {
                        acc += h[k] * input[n * ch + c];
                    }
                    k += 2;
                }
                out[m * ch + c] = acc;
            }
        }
        out
    }

    /// Amplitude of `freq` in a mono signal (Goertzel, normalised).
    fn level(x: &[f64], freq: f64, fs: f64) -> f64 {
        let w = 2.0 * PI * freq / fs;
        let (mut s1, mut s2) = (0.0, 0.0);
        for v in x {
            let s = v + 2.0 * w.cos() * s1 - s2;
            s2 = s1;
            s1 = s;
        }
        let power = s1 * s1 + s2 * s2 - 2.0 * w.cos() * s1 * s2;
        2.0 * power.max(0.0).sqrt() / x.len() as f64
    }

    /// Magnitude response of `taps` relative to the pass band gain of 2, in dB, at `len / 2 + 1`
    /// points from 0 to the output Nyquist (bin len/4 is fN).
    fn response_db(taps: &[f64], len: usize) -> Vec<f64> {
        let fwd = RealFftPlanner::<f64>::new().plan_fft_forward(len);
        let mut time = vec![0.0; len];
        time[..taps.len()].copy_from_slice(taps);
        let mut spec = fwd.make_output_vec();
        fwd.process(&mut time, &mut spec).unwrap();
        spec.iter().map(|v| 20.0 * (v.norm() / 2.0).max(1e-300).log10()).collect()
    }

    /// Worst pass-band deviation up to `pass`·fN and worst stop-band level from `stop`·fN (dB);
    /// printed with the measured transition at 88.2 kHz: from the last frequency within
    /// ±0.001 dB to the first from which everything is 150 dB down.
    fn measure(f: &DoublerFilter, pass: f64, stop: f64) -> (f64, f64) {
        let len = (f.taps().len() * 8).max(1 << 16);
        let r = response_db(f.taps(), len);
        let fn_bin = len as f64 / 4.0;
        let ripple = r[..=(pass * fn_bin).floor() as usize].iter().fold(0.0f64, |m, v| m.max(v.abs()));
        let floor = r[(stop * fn_bin).ceil() as usize..].iter().fold(-400.0f64, |m, &v| m.max(v));
        let flat = r.iter().position(|v| v.abs() > 0.001).unwrap_or(r.len());
        let deep = r.iter().rposition(|&v| v > -150.0).map_or(0, |i| i + 1);
        let hz = |bins: usize| bins as f64 * 88_200.0 / len as f64;
        println!(
            "{:?} {}: ripple ±{ripple:.1e} dB to {:.5}·fN, stop {:.1} dB from {:.5}·fN; flat (±0.001 dB) to {:.1} Hz, −150 dB from {:.1} Hz at 88.2k: transition {:.2} Hz (designed {:.2} Hz)",
            f.spec().kind,
            f.taps().len(),
            pass,
            -floor,
            stop,
            hz(flat),
            hz(deep),
            hz(deep.saturating_sub(flat)),
            (f.stopband() - f.passband()) * 22_050.0,
        );
        (ripple, floor)
    }

    fn branch_sums(f: &DoublerFilter) -> [f64; 2] {
        [0, 1].map(|b| f.taps().iter().skip(b).step_by(2).sum())
    }

    #[test]
    fn streaming_equals_direct_convolution_in_any_chunking() {
        let ch = 2;
        let input = noise(10_000 * ch, 1);
        for kind in [FilterKind::Linear, FilterKind::Minimum] {
            let f = Arc::new(DoublerFilter::design(spec(kind, 4096)));
            assert!(f.partitions() > 1);
            let want = direct(f.taps(), f.delay(), &input, ch);
            let mut first: Option<Vec<f64>> = None;
            for chunk in [1, 777, 4096, input.len()] {
                let out = run(&mut Doubler::new(f.clone(), ch), &input, chunk);
                assert_eq!(out.len(), want.len());
                let err = out.iter().zip(&want).fold(0.0f64, |m, (a, b)| m.max((a - b).abs()));
                assert!(err < 1e-9, "{kind:?}, chunks of {chunk}: error {err:e}");
                if chunk == 1 {
                    println!("{kind:?} 4096 streaming vs direct: max error {err:.1e}");
                }
                match &first {
                    Some(o) => assert!(o == &out, "{kind:?}: chunks of {chunk} differ"),
                    None => first = Some(out),
                }
            }
        }
    }

    #[test]
    fn output_is_exactly_twice_the_input() {
        for (kind, taps) in [(FilterKind::Linear, 4096), (FilterKind::Linear, 65_536), (FilterKind::Minimum, 4096), (FilterKind::Apodizing, 0)] {
            let f = Arc::new(DoublerFilter::design(spec(kind, taps)));
            for frames in [0, 1, 100, 511, 513, 1023, 5000, 20_000] {
                let input = noise(frames * 2, frames as u64);
                let out = run(&mut Doubler::new(f.clone(), 2), &input, 300);
                assert_eq!(out.len(), 4 * frames, "{kind:?} {taps}, {frames} frames");
            }
            // The first output comes after startup_frames(), no later than delay/2 + one block.
            let mut d = Doubler::new(f.clone(), 2);
            let mut out = Vec::new();
            let mut fed = 0;
            while out.is_empty() {
                d.process(&[0.1, 0.2], &mut out);
                fed += 1;
            }
            assert_eq!(fed, f.startup_frames());
            assert!(fed <= f.delay() / 2 + f.block(), "{kind:?}: {fed}");
        }
    }

    #[test]
    fn impulse_lands_on_its_time() {
        let mut x = vec![0.0; 40_000];
        x[1000] = 1.0;
        let lin = Arc::new(DoublerFilter::design(spec(FilterKind::Linear, 65_536)));
        let y = run(&mut Doubler::new(lin, 1), &x, 4096);
        let peak = y.iter().enumerate().max_by(|a, b| a.1.abs().total_cmp(&b.1.abs())).unwrap().0;
        assert_eq!(peak, 2000);
        for kind in [FilterKind::Minimum, FilterKind::Apodizing] {
            let f = Arc::new(DoublerFilter::design(spec(kind, 65_536)));
            assert_eq!(f.delay(), 0);
            let y = run(&mut Doubler::new(f, 1), &x, 4096);
            let peak = y.iter().fold(0.0f64, |m, v| m.max(v.abs()));
            let pre = y[..2000].iter().fold(0.0f64, |m, v| m.max(v.abs()));
            println!("{kind:?}: largest sample before the impulse {:.1} dB", 20.0 * (pre / peak).log10());
            assert!(pre < peak * 1e-7, "{kind:?}: pre-ringing {:.1} dB", 20.0 * (pre / peak).log10());
            assert!(y[2000].abs() > 1e-6, "{kind:?} starts late");
        }
    }

    #[test]
    fn frequency_response_meets_the_spec() {
        let lin = DoublerFilter::design(spec(FilterKind::Linear, 65_536));
        let (ripple, stop) = measure(&lin, lin.passband(), 1.0);
        assert!(ripple <= 0.001 && stop <= -145.0, "linear: ±{ripple} dB, stop {stop} dB");
        let min = DoublerFilter::design(spec(FilterKind::Minimum, 65_536));
        let (ripple, stop) = measure(&min, min.passband(), 1.0);
        assert!(ripple <= 0.01 && stop <= -145.0, "minimum: ±{ripple} dB, stop {stop} dB");
        let apo = DoublerFilter::design(spec(FilterKind::Apodizing, 0));
        let (ripple, stop) = measure(&apo, APODIZING_PASS, APODIZING_STOP);
        assert!(ripple <= 0.01 && stop <= -140.0, "apodizing: ±{ripple} dB, stop {stop} dB");
        for f in [&lin, &min, &apo] {
            for s in branch_sums(f) {
                assert!((s - 1.0).abs() < 1e-12, "{:?}: branch sums to {s}", f.spec());
            }
        }
    }

    #[test]
    fn tone_keeps_its_level_and_its_image_is_gone() {
        let f = Arc::new(DoublerFilter::design(spec(FilterKind::Linear, 65_536)));
        let x: Vec<f64> = (0..3 * 44_100).map(|i| 0.5 * (2.0 * PI * 19_000.0 * i as f64 / 44_100.0).sin()).collect();
        let y = run(&mut Doubler::new(f, 1), &x, 4096);
        // One second in the middle: whole cycles of both tones, well clear of the edges.
        let seg = &y[88_200..176_400];
        let kept = level(seg, 19_000.0, 88_200.0);
        let image = level(seg, 25_100.0, 88_200.0);
        let gain = 20.0 * (kept / 0.5).log10();
        let rejection = 20.0 * (kept / image).log10();
        println!("19 kHz through Linear 65536: {gain:+.2e} dB, image at 25.1 kHz {rejection:.1} dB down");
        assert!(gain.abs() < 0.001, "19 kHz at {gain} dB");
        assert!(rejection >= 140.0, "image only {rejection:.1} dB down");
    }

    #[test]
    fn reset_is_a_fresh_start() {
        let f = Arc::new(DoublerFilter::design(spec(FilterKind::Linear, 4096)));
        let input = noise(6000 * 2, 7);
        let fresh = run(&mut Doubler::new(f.clone(), 2), &input, 1000);
        let mut d = Doubler::new(f, 2);
        let mut junk = Vec::new();
        d.process(&noise(3333 * 2, 8), &mut junk);
        d.reset();
        assert!(run(&mut d, &input, 1000) == fresh);
        // flush leaves it reset too.
        assert!(run(&mut d, &input, 517) == fresh);
    }

    #[test]
    fn channels_are_independent() {
        let f = Arc::new(DoublerFilter::design(spec(FilterKind::Minimum, 4096)));
        let left = noise(5000, 3);
        let stereo: Vec<f64> = left.iter().flat_map(|&v| [v, 0.0]).collect();
        let out = run(&mut Doubler::new(f.clone(), 2), &stereo, 999);
        let mono = run(&mut Doubler::new(f, 1), &left, 999);
        assert!(out.iter().skip(1).step_by(2).all(|&v| v == 0.0), "right channel not silent");
        assert!(out.iter().step_by(2).zip(&mono).all(|(a, b)| a == b));
    }

    #[test]
    fn designs_are_cached_by_spec() {
        let a = filter(spec(FilterKind::Linear, 4096));
        let b = filter(spec(FilterKind::Linear, 4096));
        assert!(Arc::ptr_eq(&a, &b));
        // Apodizing ignores the length; both land on one design.
        let c = filter(spec(FilterKind::Apodizing, 0));
        let d = filter(spec(FilterKind::Apodizing, 65_536));
        assert!(Arc::ptr_eq(&c, &d));
        assert!(Arc::ptr_eq(&a, &filter(spec(FilterKind::Linear, 4096))));
        assert_eq!(spec(FilterKind::Minimum, 1 << 22).effective().taps, MAX_MINIMUM_TAPS);
        assert_eq!(spec(FilterKind::Linear, 5000).effective().taps, 8192);
        assert_eq!(spec(FilterKind::Linear, 1 << 30).effective().taps, MAX_TAPS);
    }

    #[test]
    fn shorter_steps_down_to_the_standard_filter() {
        let steps = |s: FilterSpec| std::iter::successors(Some(s), |s| s.shorter()).skip(1).map(|s| (s.kind, s.taps)).collect::<Vec<_>>();
        use FilterKind::*;
        assert_eq!(steps(spec(Linear, 1 << 24)), [(Linear, 1 << 20), (Linear, 1 << 16)]);
        assert_eq!(steps(spec(Linear, 1 << 22)), [(Linear, 1 << 20), (Linear, 1 << 16)]);
        assert_eq!(steps(spec(Linear, 1 << 18)), [(Linear, 1 << 16)]);
        assert_eq!(steps(spec(Linear, 1 << 16)), []);
        assert_eq!(steps(spec(Linear, 8192)), []);
        // Minimum is built at most 2^20 long: it steps from there.
        assert_eq!(steps(spec(Minimum, 1 << 24)), [(Minimum, 1 << 16)]);
        assert_eq!(steps(spec(Minimum, 1 << 20)), [(Minimum, 1 << 16)]);
        assert_eq!(steps(spec(Apodizing, 1 << 20)), []);
    }

    /// The response checks for 2^20 taps:
    /// `cargo test --release -p hifi-engine -- --ignored upsample`
    #[test]
    #[ignore]
    fn long_linear_response() {
        let f = DoublerFilter::design(spec(FilterKind::Linear, 1 << 20));
        let (ripple, stop) = measure(&f, f.passband(), 1.0);
        assert!(ripple <= 0.001 && stop <= -145.0, "linear 2^20: ±{ripple} dB, stop {stop} dB");
        branch_sums(&f).iter().for_each(|s| assert!((s - 1.0).abs() < 1e-12, "{s}"));
    }

    /// See [`long_linear_response`].
    #[test]
    #[ignore]
    fn long_minimum_response() {
        let f = DoublerFilter::design(spec(FilterKind::Minimum, 1 << 20));
        let (ripple, stop) = measure(&f, f.passband(), 1.0);
        assert!(ripple <= 0.01 && stop <= -145.0, "minimum 2^20: ±{ripple} dB, stop {stop} dB");
        branch_sums(&f).iter().for_each(|s| assert!((s - 1.0).abs() < 1e-12, "{s}"));
    }
}
