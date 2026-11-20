//! Design time, memory, start-up and speed of the ×2 upsampling stage, stereo, one thread; then
//! the whole upsampling chain (what the render thread runs) with the standard filter against
//! the long ones.
//! `cargo run --release -p hifi-engine --example upsample_speed`
use hifi_engine::dsp::upsample::{self, Doubler, FilterKind, FilterSpec};
use hifi_engine::dsp::{Chain, DspSettings, Upsample};
use std::time::{Duration, Instant};

const MB: f64 = 1024.0 * 1024.0;

fn main() {
    doubler();
    chain();
}

fn doubler() {
    let specs = [
        (FilterKind::Linear, 65_536),
        (FilterKind::Linear, 1 << 20),
        (FilterKind::Minimum, 65_536),
        (FilterKind::Minimum, 1 << 20),
        (FilterKind::Apodizing, 0),
        // Filter ~400 MB + 128 MB per channel of delay line: ~0.7 GB in all.
        (FilterKind::Linear, 1 << 24),
    ];
    let secs = 60.0;
    println!("{:<10} {:>9} {:>8} {:>9} {:>9} {:>7} {:>4} {:>12}   {:>16} {:>16}", "filter", "taps", "design", "filter", "doubler", "block", "P", "first out", "44.1→88.2k", "96→192k");
    for (kind, taps) in specs {
        let t = Instant::now();
        let f = upsample::filter(FilterSpec { kind, taps });
        let design = t.elapsed().as_secs_f64();
        let mut speeds = Vec::new();
        let mut doubler_mem = 0;
        for rate in [44_100.0, 96_000.0] {
            let input = vec![0.1f64; (rate * secs) as usize * 2];
            let mut d = Doubler::new(f.clone(), 2);
            doubler_mem = d.memory_bytes();
            let mut out = Vec::with_capacity(2 * 8192 + 4 * f.block());
            let t = Instant::now();
            for chunk in input.chunks(8192) {
                out.clear();
                d.process(chunk, &mut out);
            }
            speeds.push(secs / t.elapsed().as_secs_f64());
        }
        let first = f.startup_frames();
        println!(
            "{:<10} {:>9} {:>7.2}s {:>6.0} MB {:>6.0} MB {:>7} {:>4} {:>12}   {:>7.0}× ({:>4.1} s) {:>7.0}× ({:>4.1} s)",
            format!("{kind:?}"),
            f.taps().len(),
            design,
            f.memory_bytes() as f64 / MB,
            doubler_mem as f64 / MB,
            f.block(),
            f.partitions(),
            first,
            speeds[0],
            first as f64 / 44_100.0,
            speeds[1],
            first as f64 / 96_000.0,
        );
    }
}

/// The chain as the render thread drives it: 2048-frame blocks of f32 stereo, then the tail.
/// Real time factor over the whole run (start-up and tail included); the longest single call
/// while playing (a long filter's burst) and while letting the tail out at the end (a block per
/// call), each of which the ring (~0.5 s) has to cover.
fn chain() {
    let cases = [(44_100u32, 176_400u32), (44_100, 352_800), (44_100, 705_600), (96_000, 384_000)];
    let filters = [(None, "standard (HIGH)"), (Some(1 << 16), "Linear 65536"), (Some(1 << 20), "Linear 2^20")];
    let secs = 60.0;
    println!("\nWhole chain, stereo, {secs} s of input:");
    println!("{:<14} {:<16} {:>12} {:>14} {:>14}", "rates (kHz)", "filter", "real time", "longest call", "longest tail");
    let row = |from: u32, to: u32, filter: Option<usize>, name: &str, secs: f64| {
        let settings = DspSettings {
            upsample: Upsample::UpTo(to as f64),
            upsample_filter: filter.map(|taps| FilterSpec { kind: FilterKind::Linear, taps }),
            ..Default::default()
        };
        let frames = (from as f64 * secs) as usize;
        let input: Vec<f32> = (0..frames).flat_map(|i| {
            let v = (0.3 * (2.0 * std::f64::consts::PI * 997.0 * i as f64 / from as f64).sin()) as f32;
            [v, -v]
        }).collect();
        // Designed (and cached) before the clock starts, as `Playback::new` does it.
        let mut chain = Chain::new(&settings, None, None, from, to, 2);
        let (mut total, mut longest, mut tail) = (Duration::ZERO, Duration::ZERO, Duration::ZERO);
        let mut out_frames = 0;
        let mut blocks = input.chunks(2048 * 2);
        loop {
            let block = blocks.next();
            let t = Instant::now();
            let out = chain.process(block.unwrap_or(&[]), 1.0, block.is_none());
            let took = t.elapsed();
            out_frames += out.len() / 2;
            total += took;
            match block {
                Some(_) => longest = longest.max(took),
                None => tail = tail.max(took),
            }
            if block.is_none() && !chain.finishing() {
                break;
            }
        }
        assert!(out_frames as f64 >= frames as f64 * to as f64 / from as f64 * 0.999, "{out_frames}");
        println!(
            "{:<14} {:<16} {:>11.0}× {:>11.1} ms {:>11.1} ms",
            format!("{} → {}", from as f64 / 1000.0, to as f64 / 1000.0),
            name,
            secs / total.as_secs_f64(),
            longest.as_secs_f64() * 1000.0,
            tail.as_secs_f64() * 1000.0
        );
    };
    for (from, to) in cases {
        for (filter, name) in filters {
            row(from, to, filter, name, secs);
        }
    }
    // The longest there is: 101 s of input held back at 44.1 kHz, then bursts of 5.9 s.
    row(44_100, 352_800, Some(1 << 24), "Linear 2^24 *", 180.0);
    println!("* 180 s of input");
}
