//! Design time, memory, start-up and speed of the ×2 upsampling stage, stereo, one thread.
//! `cargo run --release -p hifi-engine --example upsample_speed`
use hifi_engine::dsp::upsample::{self, Doubler, FilterKind, FilterSpec};
use std::time::Instant;

const MB: f64 = 1024.0 * 1024.0;

fn main() {
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
