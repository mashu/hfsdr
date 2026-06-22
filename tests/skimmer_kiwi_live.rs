//! Live Kiwi + skimmer smoke test on 20m.
//!
//! `cargo test --test skimmer_kiwi_live -- --ignored --nocapture`

use std::thread;
use std::time::{Duration, Instant};

use hfsdr::{
    IqSource, KiwiSource, Skimmer, SkimmerConfig, SkimmerDecoderKind, SpectrumAnalyzer, SpotSort,
    KIWI_IQ_HALF_HZ,
};

#[test]
#[ignore]
fn kiwi_skimmer_decodes_on_20m() {
    let host = std::env::var("KIWI_TEST_HOST").unwrap_or_else(|_| "websdr.heppen.be".into());
    let port = std::env::var("KIWI_TEST_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8073u16);
    let center_hz: f64 = std::env::var("KIWI_TEST_HZ")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(14_030_000.0);

    let mut src = KiwiSource::new(&host, port).with_passband(-KIWI_IQ_HALF_HZ, KIWI_IQ_HALF_HZ);
    src.tune(center_hz).expect("tune");
    let mut iq = src.start().expect("start");

    let mut analyzer = SpectrumAnalyzer::new(2048, 1024);
    let mut spectrum = vec![-120.0f32; 2048];
    let mut skimmer = Skimmer::new(SkimmerConfig {
        decoder: SkimmerDecoderKind::Adaptive,
        require_scp: false,
        min_snr_db: 8.0,
        min_decode_snr_db: 6.0,
        decode_gate_ms: 35.0,
        ..SkimmerConfig::default()
    });

    let rate = src.sample_rate() as f32;
    let mut got_iq = 0usize;
    let started = Instant::now();
    let run_for = Duration::from_secs(50);
    let mut last_print = Instant::now();

    while started.elapsed() < run_for {
        thread::sleep(Duration::from_millis(40));
        let mut chunk = Vec::new();
        while let Ok(s) = iq.pop() {
            chunk.push(s);
        }
        if chunk.is_empty() {
            if let Some(err) = src.link_error() {
                panic!("kiwi error: {err}");
            }
            continue;
        }
        got_iq += chunk.len();
        analyzer.process(&chunk, |row| spectrum.copy_from_slice(row));
        skimmer.process(&chunk, rate, &spectrum, rate, 0.0, center_hz);

        if last_print.elapsed() >= Duration::from_secs(10) {
            let spots = skimmer.store().sorted(SpotSort::SnrDesc);
            eprintln!(
                "[{:.0}s] iq={got_iq} ch={} spots={}",
                started.elapsed().as_secs_f32(),
                skimmer.active_channels(),
                spots.len()
            );
            for s in spots.iter().take(8) {
                eprintln!(
                    "  {:.3} MHz SNR {:.0} {:?} {}",
                    s.frequency_hz / 1e6,
                    s.snr_db,
                    s.kind,
                    s.callsign.as_deref().unwrap_or("?")
                );
            }
            last_print = Instant::now();
        }
    }

    let spots = skimmer.store().sorted(SpotSort::SnrDesc);
    eprintln!("\n=== final: {} IQ samples, {} spots from {host} @ {:.3} MHz ===", got_iq, spots.len(), center_hz / 1e6);
    for s in spots.iter().take(25) {
        eprintln!(
            "{:.3} MHz  SNR {:>4.0}  {:?}  {}",
            s.frequency_hz / 1e6,
            s.snr_db,
            s.kind,
            s.callsign.as_deref().unwrap_or("—")
        );
    }

    assert!(
        got_iq > 50_000,
        "expected IQ stream, got {got_iq} samples (err={:?})",
        src.link_error()
    );

    if spots.is_empty() {
        eprintln!("WARNING: no spots — band may be quiet or decode thresholds need tuning");
    } else {
        let with_call = spots.iter().filter(|s| s.callsign.is_some()).count();
        eprintln!("{with_call}/{} spots have callsigns", spots.len());
        assert!(with_call > 0, "expected at least one callsign on 20m from {host}");
    }
}
