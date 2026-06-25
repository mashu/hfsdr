//! Unified engine pipeline benchmark (skeleton).
//!
//! ```text
//! cargo run --release --bin engine-bench synthetic [seconds] [sample_rate]
//! cargo run --release --bin engine-bench replay <capture.hfsdr> [seconds]
//! ```
//!
//! Live modes (`live-kiwi`, `live-airspy`) are planned — use `kiwi-pipeline-bench` /
//! `pipeline-bench` until wired here.

use std::env;
use std::time::{Duration, Instant};

use hfsdr::{
    spectrum_hop, spectrum_plan, Complex32, CwChannelSettings, FirDecimator, IqAudioDemod,
    PipelineMetrics, SpectrumAnalyzer, SpectrumFrontEnd,
};

const DEFAULT_SYNTH_RATE: u32 = 384_000;
const DEFAULT_SYNTH_SECS: f64 = 5.0;
const DRAIN_CAP: usize = 1 << 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Synthetic,
    Replay,
}

fn usage() -> ! {
    eprintln!(
        "Usage:\n  \
         engine-bench synthetic [seconds] [sample_rate_hz]\n  \
         engine-bench replay <capture.hfsdr> [seconds]\n\n\
         Env: HFSDR_PERF=1 mirrors in-app pipeline profiling."
    );
    std::process::exit(2);
}

fn parse_mode() -> (Mode, Vec<String>) {
    let mut args = env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        return (Mode::Synthetic, args);
    }
    match args[0].as_str() {
        "synthetic" => {
            args.remove(0);
            (Mode::Synthetic, args)
        }
        "replay" => {
            args.remove(0);
            (Mode::Replay, args)
        }
        "live-kiwi" | "live-airspy" => {
            eprintln!("Mode '{}' not wired yet — use kiwi-pipeline-bench / pipeline-bench.", args[0]);
            std::process::exit(1);
        }
        s if s.parse::<f64>().is_ok() => (Mode::Synthetic, args),
        _ => usage(),
    }
}

fn tone_block(len: usize, rate: f32, tone_hz: f32, amp: f32) -> Vec<Complex32> {
    (0..len)
        .map(|i| {
            let t = i as f32 / rate;
            let ph = t * tone_hz * std::f32::consts::TAU;
            Complex32::new(ph.cos() * amp, ph.sin() * amp)
        })
        .collect()
}

struct BenchState {
    drain: Vec<Complex32>,
    drain_decim: Vec<Complex32>,
    spectrum_scratch: Vec<Complex32>,
    audio_scratch: Vec<f32>,
    demod: IqAudioDemod,
    spectrum_ingress: FirDecimator,
    spectrum_front: SpectrumFrontEnd,
    analyzer: SpectrumAnalyzer,
    latest: Vec<f32>,
    cw: CwChannelSettings,
    sample_rate: f32,
    ingress_decim: usize,
    fft_size: usize,
    hop: usize,
    timers: PipelineMetrics,
    timers_avg: PipelineMetrics,
    pumps: u64,
}

impl BenchState {
    fn new(sample_rate: u32, ingress_decim: usize) -> Self {
        let rate = sample_rate as f32;
        let eff = rate / ingress_decim.max(1) as f32;
        let (spec_decim, fft_size, _) = spectrum_plan(eff, 2048, true, eff);
        let hop = spectrum_hop(fft_size, eff);
        Self {
            drain: Vec::with_capacity(DRAIN_CAP),
            drain_decim: Vec::with_capacity(DRAIN_CAP / ingress_decim.max(1)),
            spectrum_scratch: Vec::new(),
            audio_scratch: Vec::new(),
            demod: IqAudioDemod::new(),
            spectrum_ingress: FirDecimator::with_factor(
                rate,
                ingress_decim.max(1),
                true,
                hfsdr::DecimFilterKind::LinearFir,
            ),
            spectrum_front: SpectrumFrontEnd::new(rate, spec_decim, 0.0),
            analyzer: SpectrumAnalyzer::new(fft_size, hop),
            latest: vec![-120.0; fft_size],
            cw: CwChannelSettings::default(),
            sample_rate: rate,
            ingress_decim: ingress_decim.max(1),
            fft_size,
            hop,
            timers: PipelineMetrics::default(),
            timers_avg: PipelineMetrics::default(),
            pumps: 0,
        }
    }

    fn pump(&mut self, iq: &[Complex32]) {
        let mut m = PipelineMetrics::default();
        let t0 = Instant::now();
        self.drain.clear();
        self.drain.extend_from_slice(iq);
        m.drain_ns = t0.elapsed().as_nanos() as u64;
        m.got_samples = self.drain.len();

        let t1 = Instant::now();
        self.demod
            .process(&self.drain, self.sample_rate, &self.cw, &mut self.audio_scratch);
        m.demod_ns = t1.elapsed().as_nanos() as u64;

        let t2 = Instant::now();
        if self.ingress_decim > 1 {
            self.spectrum_ingress
                .decimate_block(&self.drain, &mut self.drain_decim, false);
        }
        m.ingress_ns = t2.elapsed().as_nanos() as u64;

        let fft_cap = (self.hop * 2 + self.fft_size).min(20_480);
        let base = if self.ingress_decim > 1 {
            let d = &self.drain_decim;
            if d.len() > fft_cap {
                &d[d.len() - fft_cap..]
            } else {
                d
            }
        } else if self.drain.len() > fft_cap {
            &self.drain[self.drain.len() - fft_cap..]
        } else {
            &self.drain[..]
        };

        let t3 = Instant::now();
        let (_, spec_decim, _) =
            spectrum_plan(self.sample_rate, self.fft_size, true, self.sample_rate);
        let fft_input: &[Complex32] = if spec_decim > 1 {
            self.spectrum_front.process(base, &mut self.spectrum_scratch);
            &self.spectrum_scratch
        } else {
            base
        };
        m.spectrum_front_ns = t3.elapsed().as_nanos() as u64;

        let t4 = Instant::now();
        let rows = self.analyzer.process_limited(fft_input, 4, |row| {
            self.latest.copy_from_slice(row);
        });
        m.fft_ns = t4.elapsed().as_nanos() as u64;
        m.fft_rows = rows;

        self.timers_avg.blend(&m, 0.12);
        self.accumulate(&m);
        self.pumps += 1;
    }

    fn accumulate(&mut self, m: &PipelineMetrics) {
        self.timers.drain_ns += m.drain_ns;
        self.timers.demod_ns += m.demod_ns;
        self.timers.ingress_ns += m.ingress_ns;
        self.timers.spectrum_front_ns += m.spectrum_front_ns;
        self.timers.fft_ns += m.fft_ns;
        self.timers.fft_rows += m.fft_rows;
        self.timers.got_samples += m.got_samples;
    }
}

fn run_synthetic(args: &[String]) {
    let run_secs: f64 = args
        .first()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_SYNTH_SECS);
    let sample_rate: u32 = args
        .get(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_SYNTH_RATE);
    let ingress_decim = if sample_rate > 96_000 { 4 } else { 1 };

    eprintln!(
        "engine-bench synthetic: {run_secs}s @ {sample_rate} Hz (ingress ×{ingress_decim})"
    );

    let mut bench = BenchState::new(sample_rate, ingress_decim);
    let block = tone_block(DRAIN_CAP.min(32_768), sample_rate as f32, 700.0, 0.25);
    let start = Instant::now();
    while start.elapsed().as_secs_f64() < run_secs {
        bench.pump(&block);
    }
    print_report(&bench, start.elapsed(), sample_rate);
}

fn run_replay(args: &[String]) {
    let path = args.first().map(String::as_str).unwrap_or_else(|| usage());
    let run_secs: f64 = args
        .get(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_SYNTH_SECS);

    let playback = match hfsdr::IqPlayback::open(std::path::Path::new(path)) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("replay open failed: {e}");
            std::process::exit(1);
        }
    };
    let meta = playback.meta();
    eprintln!(
        "engine-bench replay: {} @ {} Hz for {run_secs}s",
        path, meta.sample_rate
    );

    let mut bench = BenchState::new(meta.sample_rate, 1);
    let mut playback = playback;
    let start = Instant::now();
    let mut scratch = Vec::with_capacity(DRAIN_CAP);
    while start.elapsed().as_secs_f64() < run_secs {
        scratch.clear();
        while scratch.len() < DRAIN_CAP {
            match playback.pop() {
                Some(s) => scratch.push(s),
                None => break,
            }
        }
        if scratch.is_empty() {
            if playback.finished() {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
            continue;
        }
        bench.pump(&scratch);
    }
    print_report(&bench, start.elapsed(), meta.sample_rate);
}

fn print_report(bench: &BenchState, elapsed: Duration, nominal_rate: u32) {
    let pumps = bench.pumps.max(1);
    let total_ns = bench.timers.measured_total_ns().max(1);
    let sps = bench.timers.got_samples as f64 / elapsed.as_secs_f64();
    eprintln!("\n=== engine-bench ({:.2}s, {pumps} pumps) ===", elapsed.as_secs_f64());
    eprintln!(
        "throughput: {sps:.0} samples/s ({:.1}% of {nominal_rate})",
        sps / nominal_rate as f64 * 100.0
    );
    eprintln!("fft rows: {}", bench.timers.fft_rows);
    eprintln!("smoothed last pump: {:.0} µs", bench.timers_avg.measured_total_ns() as f64 / 1000.0);
    for (name, ns) in bench.timers.stage_rows() {
        if ns == 0 {
            continue;
        }
        let pct = ns as f64 / total_ns as f64 * 100.0;
        let per_pump_us = ns as f64 / pumps as f64 / 1000.0;
        eprintln!("  {name:14} {pct:5.1}%  {per_pump_us:8.1} µs/pump");
    }
    eprintln!(
        "\nJSON: {}",
        json_line(&bench.timers_avg, pumps, sps, nominal_rate)
    );
}

fn json_line(m: &PipelineMetrics, pumps: u64, sps: f64, nominal_rate: u32) -> String {
    format!(
        "{{\"pumps\":{pumps},\"sps\":{sps:.1},\"nominal_rate\":{nominal_rate},\
         \"total_us\":{:.1},\"drain_us\":{:.1},\"demod_us\":{:.1},\"ingress_us\":{:.1},\
         \"fft_us\":{:.1},\"fft_rows\":{}}}",
        m.measured_total_ns() as f64 / 1000.0,
        m.drain_ns as f64 / 1000.0,
        m.demod_ns as f64 / 1000.0,
        m.ingress_ns as f64 / 1000.0,
        m.fft_ns as f64 / 1000.0,
        m.fft_rows,
    )
}

fn main() {
    let (mode, args) = parse_mode();
    match mode {
        Mode::Synthetic => run_synthetic(&args),
        Mode::Replay => run_replay(&args),
    }
}
