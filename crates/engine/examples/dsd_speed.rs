//! Speed of the PCM → DSD encoder alone, stereo, one thread: for each rate and modulator
//! order, 30 s of a 997 Hz tone fed in 2048-frame blocks (what the render thread hands it).
//! Real-time factor over the whole run, and the longest single call against the time that
//! block lasts.
//! `cargo run --release -p hifi-engine --example dsd_speed`
use hifi_engine::dsp::dsd_encode::{DsdOut, DsdRate, Encoder, ModOrder};
use std::time::{Duration, Instant};

const BLOCK: usize = 2048;

fn main() {
    let secs = 30.0;
    println!("{:<8} {:>6} {:>11} {:>14} {:>12}", "rate", "order", "real time", "longest call", "block lasts");
    for rate in DsdRate::ALL {
        for order in ModOrder::ALL {
            let wire = rate.wire_rate() as f64;
            let frames = (secs * wire) as usize;
            // Designed (and cached) before the clock starts, as the player would.
            let mut enc = Encoder::new(DsdOut { rate, order }, 2);
            let mut pcm = Vec::with_capacity(2 * BLOCK);
            let mut words = Vec::with_capacity(2 * BLOCK);
            let (mut total, mut longest) = (Duration::ZERO, Duration::ZERO);
            let mut i = 0;
            while i < frames {
                let n = BLOCK.min(frames - i);
                pcm.clear();
                pcm.extend((i..i + n).flat_map(|f| {
                    let v = (0.5 * (2.0 * std::f64::consts::PI * 997.0 * f as f64 / wire).sin()) as f32;
                    [v, -v]
                }));
                words.clear();
                let t = Instant::now();
                enc.process(&pcm, &mut words);
                let took = t.elapsed();
                assert_eq!(words.len(), pcm.len());
                total += took;
                longest = longest.max(took);
                i += n;
            }
            assert_eq!(enc.overloads(), 0);
            println!(
                "{:<8} {:>6} {:>10.1}× {:>11.3} ms {:>9.2} ms",
                format!("DSD{}", rate.multiple()),
                order.get(),
                secs / total.as_secs_f64(),
                longest.as_secs_f64() * 1000.0,
                BLOCK as f64 / wire * 1000.0
            );
        }
    }
}
