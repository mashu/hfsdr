//! Unified engine pipeline benchmark.
//!
//! ```text
//! cargo run --release --features gui-core --bin engine-bench engine [seconds] [device_rate]
//! cargo run --release --features gui-core --bin engine-bench demod [rate_hz] [block_size] [iterations]
//! cargo run --release --features gui-core --bin engine-bench synthetic [seconds] [sample_rate]
//! cargo run --release --features gui-core --bin engine-bench replay <capture.hfsdr> [seconds]
//! cargo run --release --features gui-core --bin engine-bench live-kiwi [host] [port] [center_hz] [seconds]
//! cargo run --release --features airspy,gui-core --bin engine-bench live-airspy [seconds] [sample_rate]
//! ```
//!
//! Shares waterfall `engine` / `source` modules with the GUI binary but only exercises a
//! subset — suppress dead-code noise for the rest.
#![allow(dead_code)]

mod audio;
mod engine;
mod log;
mod skimmer;
mod source;

use std::f32::consts::TAU;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hfsdr::{
    ChannelOffsetHz, Complex32, CwChannel, CwChannelSettings, CwStageMetrics,
    IqAudioDemod, IqSource, KiwiSource, ListenOrigin, WidebandCwIngress,
};
use rtrb::RingBuffer;

use engine::{
    demod_tail_max, wideband_tail_len, ConnState, Engine, EngineParams, EngineShared, EngineStats,
    MAX_AUDIO_SAMPLES_NARROW, MAX_AUDIO_SAMPLES_WB,
};
use source::{Connection, DeviceSource};

#[cfg(feature = "airspy")]
use hfsdr::AirspyHf;

const DEFAULT_SECS: f64 = 5.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Engine,
    Demod,
    Synthetic,
    Replay,
    LiveKiwi,
    #[cfg(feature = "airspy")]
    LiveAirspy,
}

fn usage() -> ! {
    eprintln!(
        "Usage:\n  \
         engine-bench engine [seconds] [device_rate_hz]\n  \
         engine-bench demod [rate_hz] [block_size] [iterations]\n  \
         engine-bench synthetic [seconds] [sample_rate_hz]\n  \
         engine-bench replay <capture.hfsdr> [seconds]\n  \
         engine-bench live-kiwi [host] [port] [center_hz] [seconds]\n  \
         engine-bench live-airspy [seconds] [sample_rate_hz]  (requires airspy feature)"
    );
    std::process::exit(2);
}

fn parse_mode() -> (Mode, Vec<String>) {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        return (Mode::Engine, args);
    }
    match args[0].as_str() {
        "engine" => {
            args.remove(0);
            (Mode::Engine, args)
        }
        "demod" => {
            args.remove(0);
            (Mode::Demod, args)
        }
        "synthetic" => {
            args.remove(0);
            (Mode::Synthetic, args)
        }
        "replay" => {
            args.remove(0);
            (Mode::Replay, args)
        }
        "live-kiwi" => {
            args.remove(0);
            (Mode::LiveKiwi, args)
        }
        "live-airspy" => {
            args.remove(0);
            #[cfg(not(feature = "airspy"))]
            {
                eprintln!("live-airspy requires --features airspy,gui-core");
                std::process::exit(1);
            }
            #[cfg(feature = "airspy")]
            (Mode::LiveAirspy, args)
        }
        _ => usage(),
    }
}

fn tone_iq(n: usize, rate: f32, tone_hz: f32, amp: f32) -> Vec<Complex32> {
    (0..n)
        .map(|i| {
            let t = i as f32 / rate;
            let ph = TAU * tone_hz * t;
            Complex32::new(ph.cos() * amp, ph.sin() * amp)
        })
        .collect()
}

fn mock_conn(samples: &[Complex32], device_rate: f32, ingress_decim: usize) -> Connection {
    let mut conn = Connection::mock_ring(samples, 14_010_000.0, false);
    conn.device_sample_rate = device_rate;
    conn.sample_rate = device_rate / ingress_decim.max(1) as f32;
    conn.iq_ingress_decim = ingress_decim.max(1);
    conn
}

fn run_engine_bench(args: &[String]) {
    let run_secs: f64 = args.first().and_then(|s| s.parse().ok()).unwrap_or(DEFAULT_SECS);
    let device_rate: f32 = args
        .get(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(384_000.0);
    let ingress_decim = hfsdr::ingress_decimation_from_hz(0, device_rate as u32).0;

    let block = tone_iq(65_536, device_rate, 700.0, 0.25);
    let (mut prod, cons) = RingBuffer::<Complex32>::new(block.len() * 4);
    for _ in 0..8 {
        for &s in &block {
            let _ = prod.push(s);
        }
    }

    let (_tx, rx) = channel();
    let shared = Arc::new(Mutex::new(EngineShared::default()));
    let params = Arc::new(Mutex::new(EngineParams {
        perf_trace: true,
        ..EngineParams::default()
    }));
    let cancel = Arc::new(AtomicBool::new(false));
    let mut engine = Engine::new(rx, Arc::clone(&shared), Arc::clone(&params), cancel);
    let mut conn = mock_conn(&[], device_rate, ingress_decim);
    conn.iq = cons;
    engine.conn = Some(conn);
    engine.first_iq_received = true;
    engine.set_state(ConnState::Streaming);

    eprintln!(
        "engine-bench engine: {run_secs}s @ {device_rate} Hz (ingress ×{ingress_decim}, real Engine::pump_stream)"
    );

    let start = Instant::now();
    let mut pumps = 0u64;
    while start.elapsed().as_secs_f64() < run_secs {
        for &s in &block {
            let _ = prod.push(s);
        }
        let got = engine.pump_stream();
        if got > 0 {
            pumps += 1;
        } else {
            std::thread::sleep(Duration::from_micros(500));
        }
    }

    let stats = shared.lock().expect("lock").stats.clone();
    print_engine_report(&stats, start.elapsed(), pumps, device_rate as u32);
}

fn print_engine_report(stats: &EngineStats, elapsed: Duration, pumps: u64, nominal: u32) {
    let m = &stats.pipeline_avg;
    let sps = stats.effective_sps;
    eprintln!("\n=== engine-bench ({:.2}s, {pumps} pumps) ===", elapsed.as_secs_f64());
    eprintln!(
        "effective: {sps:.0} samples/s ({:.1}% of {nominal})",
        sps / nominal as f32 * 100.0
    );
    eprintln!("source drops: {}", stats.dropped);
    eprintln!("smoothed pump: {:.0} µs", m.measured_total_ns() as f64 / 1000.0);
    for (name, ns) in m.stage_rows() {
        if ns == 0 {
            continue;
        }
        let pct = ns as f64 / m.measured_total_ns().max(1) as f64 * 100.0;
        eprintln!("  {name:14} {pct:5.1}%");
    }
    eprintln!(
        "drops: catch-up {} raw-bridge {} decim-bridge {}",
        m.iq_dropped_catchup, m.raw_ring_dropped, m.decim_ring_dropped
    );
}

fn print_stage_metrics(label: &str, m: &CwStageMetrics, iterations: u32) {
    let total = m.total_ns().max(1) as f64;
    let div = iterations.max(1) as f64;
    let per_us = total / div / 1000.0;
    eprintln!("\n--- {label} ({per_us:.1} µs/call) ---");
    eprintln!(
        "  IQ in: {} → audio out: {}",
        m.iq_samples, m.audio_samples
    );
    for (name, ns) in m.stage_rows() {
        if ns == 0 || name == "audio_chain" {
            continue;
        }
        let avg_ns = ns as f64 / div;
        let pct = avg_ns / (total / div) * 100.0;
        eprintln!(
            "  {name:14} {pct:5.1}%  ({:.1} µs)",
            avg_ns / 1000.0
        );
    }
}

fn accumulate_metrics(acc: &mut CwStageMetrics, m: &CwStageMetrics) {
    acc.noise_blanker_ns += m.noise_blanker_ns;
    acc.nco_ns += m.nco_ns;
    acc.decim_ns += m.decim_ns;
    acc.audio_chain_ns += m.audio_chain_ns;
    acc.notches_ns += m.notches_ns;
    acc.channel_filter_ns += m.channel_filter_ns;
    acc.agc_ns += m.agc_ns;
    acc.detector_ns += m.detector_ns;
    acc.polish_ns += m.polish_ns;
    acc.iq_samples = m.iq_samples;
    acc.audio_samples = m.audio_samples;
}

fn demod_slice<'a>(samples: &'a [Complex32], rate: f32) -> &'a [Complex32] {
    let max = demod_tail_max(rate);
    let len = wideband_tail_len(samples.len(), rate, max);
    if len >= samples.len() {
        samples
    } else {
        &samples[samples.len() - len..]
    }
}

fn bench_cw_channel(rate: f32, block_size: usize, iterations: u32, settings: &CwChannelSettings) {
    let iq = tone_iq(block_size, rate, 700.0, 0.2);
    let demod_iq = demod_slice(&iq, rate);
    let tail_max = demod_tail_max(rate);
    let mut channel = CwChannel::new(rate);
    let mut audio = Vec::new();
    let origin = ListenOrigin::from_settings(settings.listen_offset_hz);
    for _ in 0..50 {
        let mut m = CwStageMetrics::default();
        channel.process_profiled(demod_iq, rate, settings, origin, &mut audio, &mut m);
    }

    let mut accum = CwStageMetrics::default();
    let t0 = Instant::now();
    for _ in 0..iterations {
        let mut m = CwStageMetrics::default();
        channel.process_profiled(demod_iq, rate, settings, origin, &mut audio, &mut m);
        accumulate_metrics(&mut accum, &m);
    }
    let elapsed = t0.elapsed();

    let per_us = elapsed.as_nanos() as f64 / iterations as f64 / 1000.0;
    eprintln!(
        "\n=== CwChannel @ {rate} Hz (drain {block_size} → demod {} IQ, tail max {tail_max}, {iterations} iters, {per_us:.1} µs/call) ===",
        demod_iq.len()
    );
    print_stage_metrics("CwChannel stages (avg)", &accum, iterations);
}

fn run_demod_microbench(args: &[String]) {
    let rate: f32 = args.first().and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let block_size: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(8192);
    let iterations: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(2000);
    let economy = args.iter().any(|s| s == "economy");

    let rates: Vec<f32> = if rate > 0.0 {
        vec![rate]
    } else {
        vec![12_000.0, 384_000.0]
    };

    let mut settings = CwChannelSettings::default();
    settings.listen_offset_hz = ChannelOffsetHz::new(700.0);
    settings.economy_filter = economy;
    if economy {
        eprintln!("=== economy filter (2-pole IIR override) ===");
    }

    for &r in &rates {
        if r > 96_000.0 {
            bench_wideband_demod(r, block_size, iterations, &settings);
        }
        bench_cw_channel(r, block_size, iterations, &settings);
    }

    eprintln!(
        "\n=== policy: demod tail max narrow={MAX_AUDIO_SAMPLES_NARROW} wide={MAX_AUDIO_SAMPLES_WB} ==="
    );
}

fn bench_wideband_demod(
    rate: f32,
    block_size: usize,
    iterations: u32,
    settings: &CwChannelSettings,
) {
    let iq = tone_iq(block_size, rate, 700.0, 0.2);
    let demod_iq = demod_slice(&iq, rate);
    let mut demod = IqAudioDemod::new();
    let mut audio = Vec::new();

    for _ in 0..50 {
        demod.process(demod_iq, rate, settings, &mut audio);
    }

    let t0 = Instant::now();
    for _ in 0..iterations {
        demod.process(demod_iq, rate, settings, &mut audio);
    }
    let total_us = t0.elapsed().as_nanos() as f64 / iterations as f64 / 1000.0;

    let mut ingress = WidebandCwIngress::new(rate, settings.decimation, settings.decim_filter);
    let mut channel = CwChannel::new(ingress.audio_rate());
    let mut bb_settings = settings.clone();
    bb_settings.listen_offset_hz = ChannelOffsetHz::ZERO;
    bb_settings.decimation = 1;
    let origin = ListenOrigin::after_upstream_mix(settings.listen_offset_hz);
    let mut ingress_ns = 0u64;
    let audio_rate = ingress.audio_rate();
    let mut channel_metrics = CwStageMetrics::default();
    for _ in 0..iterations {
        let t = Instant::now();
        let bb = ingress.to_baseband(
            demod_iq,
            rate,
            settings.listen_offset_hz,
            &settings.diagnostic,
        );
        ingress_ns += t.elapsed().as_nanos() as u64;
        let mut m = CwStageMetrics::default();
        channel.process_profiled(
            bb,
            audio_rate,
            &bb_settings,
            origin,
            &mut audio,
            &mut m,
        );
        accumulate_metrics(&mut channel_metrics, &m);
    }

    let ingress_us = ingress_ns as f64 / iterations as f64 / 1000.0;
    let channel_us = channel_metrics.total_ns() as f64 / iterations as f64 / 1000.0;
    let split_us = ingress_us + channel_us;
    eprintln!(
        "\n=== IqAudioDemod wideband @ {rate} Hz (drain {block_size} → demod {} IQ, {iterations} iters) ===",
        demod_iq.len()
    );
    eprintln!("  total:        {total_us:.1} µs/call");
    eprintln!(
        "  ingress:      {ingress_us:.1} µs ({:.0}%)",
        ingress_us / split_us.max(0.001) * 100.0
    );
    eprintln!(
        "  CwChannel:    {channel_us:.1} µs ({:.0}%)",
        channel_us / split_us.max(0.001) * 100.0
    );
    eprintln!("  split sum:    {split_us:.1} µs (sanity vs total)");
    print_stage_metrics("CwChannel inside wideband", &channel_metrics, iterations);
}

fn run_synthetic_legacy(args: &[String]) {
    let _ = args;
    eprintln!("synthetic mode delegates to engine mode with mock ring");
    run_engine_bench(args);
}

fn run_replay(args: &[String]) {
    let path = args.first().map(String::as_str).unwrap_or_else(|| usage());
    let run_secs: f64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(DEFAULT_SECS);

    let playback = hfsdr::IqPlayback::open(std::path::Path::new(path)).unwrap_or_else(|e| {
        eprintln!("replay open failed: {e}");
        std::process::exit(1);
    });
    let meta = playback.meta();
    let (_tx, rx) = channel();
    let shared = Arc::new(Mutex::new(EngineShared::default()));
    let params = Arc::new(Mutex::new(EngineParams {
        perf_trace: true,
        ..EngineParams::default()
    }));
    let cancel = Arc::new(AtomicBool::new(false));
    let mut engine = Engine::new(rx, Arc::clone(&shared), Arc::clone(&params), cancel);
    engine.playback = Some(playback);
    engine.first_iq_received = true;
    engine.set_state(ConnState::Streaming);

    eprintln!("engine-bench replay: {path} for {run_secs}s");
    let start = Instant::now();
    let mut pumps = 0u64;
    while start.elapsed().as_secs_f64() < run_secs {
        let got = engine.pump_stream();
        if engine.playback.is_none() {
            break;
        }
        if got > 0 {
            pumps += 1;
        } else {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    let stats = shared.lock().expect("lock").stats.clone();
    print_engine_report(&stats, start.elapsed(), pumps, meta.sample_rate);
}

fn run_live_kiwi(args: &[String]) {
    let host = args.first().cloned().unwrap_or_else(|| "192.36.155.252".into());
    let port: u16 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(8073);
    let center: f64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(14_010_000.0);
    let run_secs: f64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(DEFAULT_SECS);

    let mut src = KiwiSource::new(host.clone(), port);
    src.tune(center).expect("tune");
    let reported = src.sample_rate();
    let (ingress_decim, eff_sr) = hfsdr::ingress_decimation_from_hz(0, reported);
    let device_iq = src.start().expect("start");
    let ring_cap = 1 << 16;
    let (iq, iq_spectrum, bridge, iq_spectrum_ring_capacity) =
        source::attach_dual_ring(
            device_iq,
            ingress_decim,
            reported as f32,
            ring_cap,
            hfsdr::DecimFilterKind::LinearFir,
        );

    let (_tx, rx) = channel();
    let shared = Arc::new(Mutex::new(EngineShared::default()));
    let params = Arc::new(Mutex::new(EngineParams {
        perf_trace: true,
        ..EngineParams::default()
    }));
    let cancel = Arc::new(AtomicBool::new(false));
    let mut engine = Engine::new(rx, Arc::clone(&shared), Arc::clone(&params), cancel);
    engine.conn = Some(Connection {
        device: DeviceSource::Kiwi(src),
        iq,
        iq_spectrum,
        bridge,
        iq_ring_capacity: ring_cap,
        iq_spectrum_ring_capacity,
        device_sample_rate: reported as f32,
        sample_rate: eff_sr,
        center_hz: center,
        is_kiwi: true,
        iq_ingress_decim: ingress_decim,
    });
    engine.first_iq_received = true;
    engine.set_state(ConnState::Streaming);

    eprintln!("engine-bench live-kiwi: {host}:{port} @ {center} Hz for {run_secs}s");
    let start = Instant::now();
    let mut pumps = 0u64;
    while start.elapsed().as_secs_f64() < run_secs {
        let got = engine.pump_stream();
        if got > 0 {
            pumps += 1;
        } else {
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    let stats = shared.lock().expect("lock").stats.clone();
    print_engine_report(&stats, start.elapsed(), pumps, reported);
    if let Some(mut conn) = engine.conn.take() {
        let _ = conn.device.stop();
    }
}

#[cfg(feature = "airspy")]
fn run_live_airspy(args: &[String]) {
    let run_secs: f64 = args.first().and_then(|s| s.parse().ok()).unwrap_or(DEFAULT_SECS);
    let sample_rate: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(384_000);

    let mut radio = AirspyHf::open().expect("airspy open");
    radio.set_sample_rate(sample_rate).expect("rate");
    radio.set_hf_agc(true).ok();
    radio.tune(14_035_000.0).expect("tune");
    let (ingress_decim, eff_sr) = hfsdr::ingress_decimation_from_hz(0, sample_rate);
    let device_iq = radio.start().expect("start");
    let ring_cap = hfsdr::airspyhf::iq_ring_capacity(sample_rate);
    let (iq, iq_spectrum, bridge, iq_spectrum_ring_capacity) =
        source::attach_dual_ring(
            device_iq,
            ingress_decim,
            sample_rate as f32,
            ring_cap,
            hfsdr::DecimFilterKind::LinearFir,
        );

    let (_tx, rx) = channel();
    let shared = Arc::new(Mutex::new(EngineShared::default()));
    let params = Arc::new(Mutex::new(EngineParams {
        perf_trace: true,
        ..EngineParams::default()
    }));
    let cancel = Arc::new(AtomicBool::new(false));
    let mut engine = Engine::new(rx, Arc::clone(&shared), Arc::clone(&params), cancel);
    engine.conn = Some(Connection {
        device: DeviceSource::Airspy(radio),
        iq,
        iq_spectrum,
        bridge,
        iq_ring_capacity: ring_cap,
        iq_spectrum_ring_capacity,
        device_sample_rate: sample_rate as f32,
        sample_rate: eff_sr,
        center_hz: 14_035_000.0,
        is_kiwi: false,
        iq_ingress_decim: ingress_decim,
    });
    engine.first_iq_received = true;
    engine.set_state(ConnState::Streaming);

    eprintln!("engine-bench live-airspy: {run_secs}s @ {sample_rate} Hz");
    let start = Instant::now();
    let mut pumps = 0u64;
    while start.elapsed().as_secs_f64() < run_secs {
        let got = engine.pump_stream();
        if got > 0 {
            pumps += 1;
        } else {
            std::thread::sleep(Duration::from_micros(500));
        }
    }
    let stats = shared.lock().expect("lock").stats.clone();
    print_engine_report(&stats, start.elapsed(), pumps, sample_rate);
    if let Some(mut conn) = engine.conn.take() {
        let _ = conn.device.stop();
    }
}

fn main() {
    let (mode, args) = parse_mode();
    match mode {
        Mode::Engine => run_engine_bench(&args),
        Mode::Demod => run_demod_microbench(&args),
        Mode::Synthetic => run_synthetic_legacy(&args),
        Mode::Replay => run_replay(&args),
        Mode::LiveKiwi => run_live_kiwi(&args),
        #[cfg(feature = "airspy")]
        Mode::LiveAirspy => run_live_airspy(&args),
    }
}
