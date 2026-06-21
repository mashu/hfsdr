//! Live Kiwi handshake test — run with `cargo test --test kiwi_live -- --ignored --nocapture`

use std::thread;
use std::time::Duration;

use hfsdr::{IqSource, KiwiSource, KIWI_IQ_HALF_HZ};

#[test]
#[ignore]
fn kiwi_receives_iq_samples() {
    let host = std::env::var("KIWI_TEST_HOST").unwrap_or_else(|_| "oh1ct.sytes.net".to_string());
    let port = std::env::var("KIWI_TEST_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8073u16);
    let mut src = KiwiSource::new(host, port)
        .with_passband(-KIWI_IQ_HALF_HZ, KIWI_IQ_HALF_HZ);
    src.tune(14_030_000.0).expect("tune");
    let mut iq = src.start().expect("start");

    let mut got = 0usize;
    for _ in 0..80 {
        thread::sleep(Duration::from_millis(100));
        while iq.pop().is_ok() {
            got += 1;
        }
        if got > 512 {
            break;
        }
    }

    assert!(
        got > 512 || src.link_error().is_some(),
        "expected IQ samples, got {got} (error={:?})",
        src.link_error()
    );
    if got > 512 {
        let rssi = src.rssi_dbm().unwrap_or(0.0);
        eprintln!("kiwi live: {got} samples, S-meter {rssi:.1} dBm");
    } else {
        eprintln!("kiwi live: server busy or rejected: {:?}", src.link_error());
    }
}
