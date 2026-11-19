//! Hardware check of the blind test's switch: prepare two files, play one, and swap between
//! them a few times on a real device.
//! Usage: abx_check <device-name-part> <file A> <file B> [shared]
use std::time::Duration;

use hifi_engine::abx;
use hifi_engine::output::{self, PlayOptions, Player};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dev = args[0].to_lowercase();
    let shared = args.get(3).is_some_and(|s| s == "shared");
    let device = output::output_devices()?.into_iter().find(|d| d.name.to_lowercase().contains(&dev)).expect("device");
    let pair = abx::of_files(std::path::Path::new(&args[1]), std::path::Path::new(&args[2]))?;
    println!("{:#?}", pair.prepared);
    let sleep = |ms| std::thread::sleep(Duration::from_millis(ms));
    let p = Player::start(device.device, pair.a.clone(), &PlayOptions { exclusive: !shared, ..Default::default() })?;
    println!("rate {} → {}, bit-perfect {}", p.path.source_rate, p.path.device_rate, p.path.bit_perfect);
    sleep(500);
    for (i, t) in [&pair.b, &pair.a, &pair.a, &pair.b, &pair.b, &pair.a].into_iter().enumerate() {
        let before = p.position_secs();
        let ok = p.swap(t.clone());
        sleep(400);
        let heard = std::sync::Arc::ptr_eq(&p.heard(), t);
        println!("swap {i}: accepted {ok}, heard the asked one {heard}, moved {:.3} s in 0.4 s", p.position_secs() - before);
    }
    println!("underruns {}, finished {}", p.underruns(), p.is_finished());
    Ok(())
}
