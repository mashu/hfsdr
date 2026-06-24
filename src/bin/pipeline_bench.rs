//! Headless Airspy pipeline profiler — measures engine hot-path cost at 20m.
//!
//! Usage: `cargo run --release --features airspy --bin pipeline-bench [seconds]`

use std::time::{Duration, Instant};

use hfsdr::{
    spectrum_hop, spectrum_plan, Complex32, CwChannelSettings, FirDecimator, IqAudioDemod,
    IqSource, SpectrumAnalyzer, SpectrumFrontEnd, AirspyHf,
};

const CENTER_HZ: f64 = 14_035_000.0;
const SAMPLE_RATE: u32 = 384_000;
const WARMUP_SECS: f64 = 0.5;

#[derive(Default, Clone, Copy)]
struct Timers {
    drain_ns: u64,
    audio_ns: u64,
    ingress_decim_ns: u64,
    spectrum_front_ns: u64,
    fft_ns: u64,
    pumps: u64,
    samples_drained: u64,
    fft_rows: u64,
    drops_start: u64,
    drops_end: u64,
}

fn main() {
    let run_secs: f64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(3.0);

    let mut radio = match AirspyHf::open() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Airspy open failed: {e}");
            std::process::exit(1);
        }
    };
    radio.set_sample_rate(SAMPLE_RATE).expect("sample rate");
    radio.set_lib_dsp(true).ok();
    radio.set_hf_agc(true).ok();
    radio.tune(CENTER_HZ).expect("tune");
    let mut iq = radio.start().expect("start");
    let _drops_start = radio.dropped_samples();

    let mut drain = Vec::with_capacity(1 << 17);
    let mut drain_decim = Vec::with_capacity(1 << 16);
    let mut spectrum_scratch = Vec::new();
    let mut audio_scratch = Vec::new();
    let mut demod = IqAudioDemod::new();
    let cw = CwChannelSettings::default();

    let view_span = SAMPLE_RATE as f32;
    let (spec_decim, fft_size, eff_rate) =
        spectrum_plan(SAMPLE_RATE as f32, 2048, true, view_span);
    let hop = spectrum_hop(fft_size, SAMPLE_RATE as f32);
    let mut spectrum_front = SpectrumFrontEnd::new(SAMPLE_RATE as f32, spec_decim, 0.0);
    let mut spectrum_ingress = FirDecimator::with_factor(SAMPLE_RATE as f32, 1, true, hfsdr::DecimFilterKind::LinearFir);
    let mut analyzer = SpectrumAnalyzer::new(fft_size, hop);
    let mut latest = vec![-120.0; fft_size];

    eprintln!(
        "20m bench @ {SAMPLE_RATE} Hz, center {CENTER_HZ} Hz, fft={fft_size} hop={hop} spec_decim={spec_decim} eff={eff_rate}"
    );

    let warmup = Instant::now();
    while warmup.elapsed().as_secs_f64() < WARMUP_SECS {
        while iq.pop().is_ok() {}
        std::thread::sleep(Duration::from_millis(1));
    }

    let mut t = Timers::default();
    let start = Instant::now();
    while start.elapsed().as_secs_f64() < run_secs {
        let pump_start = Instant::now();

        let t0 = Instant::now();
        drain.clear();
        const DRAIN_CAP: usize = 1 << 17;
        while drain.len() < DRAIN_CAP {
            match iq.pop() {
                Ok(s) => drain.push(s),
                Err(_) => break,
            }
        }
        let got = drain.len();
        t.drain_ns += t0.elapsed().as_nanos() as u64;
        t.samples_drained += got as u64;
        if got == 0 {
            std::thread::sleep(Duration::from_micros(500));
            continue;
        }

        let audio_iq = &drain[..];
        let t1 = Instant::now();
        demod.process(audio_iq, SAMPLE_RATE as f32, &cw, &mut audio_scratch);
        t.audio_ns += t1.elapsed().as_nanos() as u64;

        let t2 = Instant::now();
        spectrum_ingress.decimate_block(&drain, &mut drain_decim, false);
        t.ingress_decim_ns += t2.elapsed().as_nanos() as u64;

        let fft_cap = (hop * 2 + fft_size).min(20_480);
        let base = if drain_decim.len() > fft_cap {
            &drain_decim[drain_decim.len() - fft_cap..]
        } else {
            &drain_decim[..]
        };

        let t3 = Instant::now();
        let fft_input: &[Complex32] = if spec_decim > 1 {
            spectrum_front.process(base, &mut spectrum_scratch);
            &spectrum_scratch
        } else {
            base
        };
        t.spectrum_front_ns += t3.elapsed().as_nanos() as u64;

        let t4 = Instant::now();
        let rows = analyzer.process_limited(fft_input, 2, |row| {
            latest.copy_from_slice(row);
        });
        t.fft_ns += t4.elapsed().as_nanos() as u64;
        t.fft_rows += rows as u64;

        t.pumps += 1;
        let _ = pump_start;
    }

    t.drops_end = radio.dropped_samples();
    radio.stop().ok();

    // Fixed-size microbench (isolates per-chunk cost).
    let mut synth = vec![Complex32 { re: 0.01, im: 0.0 }; 8192];
    for (i, s) in synth.iter_mut().enumerate() {
        let t = i as f32 / SAMPLE_RATE as f32;
        s.re += (t * 800.0 * std::f32::consts::TAU).sin() * 0.02;
    }
    let n = 2000u32;
    let t0 = Instant::now();
    for _ in 0..n {
        demod.process(&synth, SAMPLE_RATE as f32, &cw, &mut audio_scratch);
    }
    let audio_8192_us = t0.elapsed().as_nanos() as f64 / n as f64 / 1000.0;

    let elapsed = start.elapsed().as_secs_f64();
    let total_ns = t.drain_ns + t.audio_ns + t.ingress_decim_ns + t.spectrum_front_ns + t.fft_ns;
    let sps = t.samples_drained as f64 / elapsed;

    eprintln!("\n=== pipeline bench ({elapsed:.2}s, {} pumps) ===", t.pumps);
    eprintln!("throughput: {sps:.0} samples/s ({:.1}% of {SAMPLE_RATE})", sps / SAMPLE_RATE as f64 * 100.0);
    eprintln!("drops during run: {}", t.drops_end.saturating_sub(t.drops_start));
    eprintln!("fft rows emitted: {}", t.fft_rows);
    for (name, ns) in [
        ("drain", t.drain_ns),
        ("audio (full drain)", t.audio_ns),
        ("ingress copy", t.ingress_decim_ns),
        ("spectrum front", t.spectrum_front_ns),
        ("fft (2 rows)", t.fft_ns),
    ] {
        let pct = ns as f64 / total_ns as f64 * 100.0;
        let per_pump_us = ns as f64 / t.pumps as f64 / 1000.0;
        eprintln!("  {name:16} {pct:5.1}%  {per_pump_us:8.1} µs/pump");
    }
    eprintln!("  total measured   {:.1} ms/s wall", total_ns as f64 / 1e9 / elapsed);
    eprintln!("\nfixed 8192-sample audio: {audio_8192_us:.1} µs/call (target <275 for 10× vs legacy ~2750)");
}
