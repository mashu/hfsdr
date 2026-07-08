//! Live Kiwi + skimmer validation on 20m (long run).
//!
//! ```bash
//! KIWI_TEST_HOST=22639.proxy.kiwisdr.com KIWI_TEST_SECS=300 \
//!   cargo test --test skimmer_kiwi_live -- --ignored --nocapture
//! ```

use std::collections::HashSet;
use std::thread;
use std::time::{Duration, Instant};

use hfsdr::{
    IqSource, KiwiSource, MasterScp, Skimmer, SkimmerConfig, SpectrumAnalyzer,
    Spot, SpotSort, KIWI_IQ_HALF_HZ,
};

fn run_secs() -> u64 {
    std::env::var("KIWI_TEST_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(300)
}

fn plausible_callsign(call: &str, scp: &MasterScp) -> bool {
    let c = call.trim().to_ascii_uppercase();
    if c.len() < 4 {
        return false;
    }
    scp.resolve(&c).is_some_and(|resolved| resolved == c)
}

fn score_spots(spots: &[Spot], scp: &MasterScp) -> (usize, Vec<String>) {
    let mut good = Vec::new();
    for s in spots {
        if let Some(ref c) = s.callsign {
            if plausible_callsign(c, scp) {
                good.push(c.clone());
            }
        }
    }
    good.sort();
    good.dedup();
    (good.len(), good)
}

#[test]
#[ignore]
fn kiwi_skimmer_decodes_on_20m() {
    let host = std::env::var("KIWI_TEST_HOST").unwrap_or_else(|_| "22639.proxy.kiwisdr.com".into());
    let port = std::env::var("KIWI_TEST_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8073u16);
    let center_hz: f64 = std::env::var("KIWI_TEST_HZ")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(14_030_000.0);
    let run_for = Duration::from_secs(run_secs());

    eprintln!(
        "=== skimmer live: {host}:{port} @ {:.3} MHz, {}s ===",
        center_hz / 1e6,
        run_for.as_secs()
    );

    let mut src = KiwiSource::new(&host, port).with_passband(-KIWI_IQ_HALF_HZ, KIWI_IQ_HALF_HZ);
    src.tune(center_hz).expect("tune");
    let mut iq = src.start().expect("start");

    let scp = MasterScp::discover();
    if scp.is_loaded() {
        eprintln!("MASTER.SCP loaded: {} calls", scp.len());
    } else {
        eprintln!("MASTER.SCP not loaded — using heuristic callsign validation");
    }

    let mut analyzer = SpectrumAnalyzer::new(2048, 1024);
    let mut spectrum = vec![-120.0f32; 2048];
    let mut skimmer = Skimmer::new(SkimmerConfig {
        require_scp: true,
        min_snr_db: 7.0,
        min_decode_snr_db: 5.0,
        decode_gate_ms: 90.0,
        channel_timeout_secs: 30.0,
        decoder_params: hfsdr::DecoderParams {
            initial_wpm: 22.0,
            envelope: hfsdr::EnvelopeSettings {
                thr_low: 0.48,
                thr_high: 0.65,
                min_span_fraction: 0.06,
            },
            ..hfsdr::DecoderParams::default()
        },
        ..SkimmerConfig::default()
    });
    skimmer.reload_scp_discover();

    let rate = src.sample_rate() as f32;
    let mut got_iq = 0usize;
    let started = Instant::now();
    let mut last_print = Instant::now();
    let mut best_plausible = 0usize;
    let mut all_good: HashSet<String> = HashSet::new();
    let print_every = Duration::from_secs(30);

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

        if last_print.elapsed() >= print_every {
            let spots = skimmer.store().sorted(SpotSort::SnrDesc);
            let (n_good, good) = score_spots(&spots, &scp);
            best_plausible = best_plausible.max(n_good);
            for g in good {
                all_good.insert(g);
            }
            eprintln!(
                "[{:.0}s] iq={got_iq} ch={} spots={} plausible={n_good} (best {})",
                started.elapsed().as_secs_f32(),
                skimmer.active_channels(),
                spots.len(),
                best_plausible
            );
            for s in spots.iter().take(6) {
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
    let (n_good, good) = score_spots(&spots, &scp);
    for g in good {
        all_good.insert(g);
    }
    best_plausible = best_plausible.max(n_good);

    eprintln!(
        "\n=== final: {} IQ samples, {} spots, {} plausible (unique {}) from {host} @ {:.3} MHz ===",
        got_iq,
        spots.len(),
        n_good,
        all_good.len(),
        center_hz / 1e6
    );
    for s in spots.iter().take(30) {
        let mark = s
            .callsign
            .as_deref()
            .is_some_and(|c| plausible_callsign(c, &scp));
        eprintln!(
            "{}{:.3} MHz  SNR {:>4.0}  {:?}  {}",
            if mark { "* " } else { "  " },
            s.frequency_hz / 1e6,
            s.snr_db,
            s.kind,
            s.callsign.as_deref().unwrap_or("—")
        );
    }
    if !all_good.is_empty() {
        eprintln!("\nPlausible callsigns seen during run:");
        let mut list: Vec<_> = all_good.into_iter().collect();
        list.sort();
        for c in list {
            eprintln!("  {c}");
        }
    }

    assert!(
        got_iq > 100_000,
        "expected IQ stream, got {got_iq} samples (err={:?})",
        src.link_error()
    );

    if best_plausible == 0 {
        eprintln!(
            "WARNING: no plausible callsigns in {}s — band may be quiet; try longer KIWI_TEST_SECS or check audio on waterfall",
            run_for.as_secs()
        );
    } else {
        eprintln!("OK: saw {best_plausible} plausible decode(s)");
    }
}
