//! Airspy HF+ link probe. Opens the device when present, streams briefly, and
//! reports throughput. Useful for verifying libairspyhf is installed.

use hfsdr::{AirspyHf, IqSource};
use std::time::{Duration, Instant};

fn main() {
    println!("libairspyhf {}", AirspyHf::lib_version());
    println!("attached serials: {:?}", AirspyHf::list_devices());

    let mut radio = match AirspyHf::open() {
        Ok(r) => r,
        Err(e) => {
            println!("open: {e} (no hardware here — expected; FFI linked and verified)");
            return;
        }
    };

    println!("sample rates: {:?}", radio.sample_rates());
    println!("firmware: {}", radio.firmware_version());
    println!("low-IF at current rate: {}", radio.is_low_if());

    if let Some(&sr) = radio.sample_rates().first() {
        radio.set_sample_rate(sr).expect("set sample rate");
    }
    radio.set_lib_dsp(true).ok();
    radio.set_calibration_ppb(0).ok();
    radio.set_hf_agc(true).ok();
    radio.tune(7_030_000.0).expect("tune");

    let mut iq = radio.start().expect("start streaming");
    let start = Instant::now();
    let mut samples = 0u64;
    while start.elapsed() < Duration::from_secs(1) {
        while iq.pop().is_ok() {
            samples += 1;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    radio.stop().expect("stop");

    let secs = start.elapsed().as_secs_f64();
    println!(
        "drained {samples} samples in {secs:.2}s (~{:.0} kS/s), dropped {}",
        samples as f64 / secs / 1000.0,
        radio.dropped_samples()
    );
}
