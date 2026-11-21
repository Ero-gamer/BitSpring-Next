//! How fast files decode through the engine (whole file, one thread), in times real time.
//! `cargo run --release -p hifi-engine --example decode_speed -- <file>...`
use std::time::Instant;

fn main() -> anyhow::Result<()> {
    for file in std::env::args().skip(1) {
        let t = Instant::now();
        let track = hifi_engine::load_track(std::path::Path::new(&file))?;
        let took = t.elapsed().as_secs_f64();
        let secs = track.duration_secs();
        println!("{file}: {:.1} s of {} Hz, {} ch in {:.3} s = {:.0}x real time", secs, track.sample_rate, track.channels, took, secs / took);
    }
    Ok(())
}
