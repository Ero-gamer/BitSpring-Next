//! Sum an album's audio the way the CUETools Database does, and print what a lookup needs:
//!   cargo run -p hifi-engine --release --example rip_check -- <track files in disc order>
//! With a saved answer of the database (its XML) as RIP_XML, compares the two.

use hifi_engine::ripcheck;

fn main() -> anyhow::Result<()> {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    let (toc, sums) = match ripcheck::read(&paths, |n| eprintln!("track {n} read"))? {
        Ok(r) => r,
        Err(unfit) => anyhow::bail!("not a CD rip: {unfit:?}"),
    };
    println!("toc {}", toc.ctdb());
    println!("disc {:08x}", sums.disc_crc(0).unwrap());
    for i in 0..sums.tracks() {
        println!("track {} {:08x}", i + 1, sums.track_crc(i, 0, false).unwrap());
    }
    if let Ok(file) = std::env::var("RIP_XML") {
        let entries = ripcheck::parse_entries(&std::fs::read_to_string(file)?);
        println!("{} entries", entries.len());
        println!("{:?}", ripcheck::compare(&sums, &entries));
    }
    Ok(())
}
