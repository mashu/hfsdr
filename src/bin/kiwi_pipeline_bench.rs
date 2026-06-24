//! Headless KiwiSDR full-pipeline profiler — 20m band with skimmer enabled.
//!
//! Mirrors the waterfall engine hot path (drain → demod → spectrum → FFT → skimmer)
//! against a live Kiwi link. Reports per-stage CPU time and process utilization.
//!
//! Usage:
//!   cargo run --release --bin kiwi-pipeline-bench [host] [port] [center_hz] [seconds]
//!
//! Defaults: 192.36.155.252 8073 14010000 15

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TryRecvError};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use hfsdr::{
    kiwi_iq_half_hz, Complex32, CwChannelSettings, FirDecimator, IqAudioDemod, IqSource,
    KiwiSource, Skimmer, SkimmerConfig, SkimmerDecoderKind, SpectrumAnalyzer, SpectrumFrontEnd,
    spectrum_hop, spectrum_plan,
};

const WARMUP_SECS: f64 = 3.0;
const MAX_DRAIN: usize = 1 << 16;
const FFT_SIZE: usize = 2048;

/// Matches typical saved Kiwi settings (20m, ~5 kHz passband).
struct KiwiOpts {
    iq_rate_hz: u32,
    iq_half_bw_hz: u32,
}

impl Default for KiwiOpts {
    fn default() -> Self {
        Self {
            iq_rate_hz: 20_250,
            iq_half_bw_hz: 2_500,
        }
    }
}

#[derive(Default, Clone, Copy)]
struct Timers {
    drain_ns: u64,
    demod_ns: u64,
    ingress_ns: u64,
    spectrum_front_ns: u64,
    fft_ns: u64,
    skimmer_submit_ns: u64,
    pumps: u64,
    samples: u64,
    fft_rows: u64,
}

struct SkimmerInput {
    iq: Vec<Complex32>,
    spectrum: Vec<f32>,
    iq_rate: f32,
    spectrum_rate: f32,
    center_hz: f64,
}

enum SkimmerMsg {
    Frame(SkimmerInput),
}

struct SkimmerWorker {
    tx: SyncSender<SkimmerMsg>,
    process_ns: Arc<AtomicU64>,
    channels: Arc<AtomicU64>,
    spots: Arc<AtomicU64>,
    join: Option<thread::JoinHandle<()>>,
}

impl SkimmerWorker {
    fn spawn(config: SkimmerConfig) -> Self {
        let (tx, rx) = sync_channel::<SkimmerMsg>(32);
        let process_ns = Arc::new(AtomicU64::new(0));
        let channels = Arc::new(AtomicU64::new(0));
        let spots = Arc::new(AtomicU64::new(0));
        let process_ns_t = Arc::clone(&process_ns);
        let channels_t = Arc::clone(&channels);
        let spots_t = Arc::clone(&spots);
        let join = thread::Builder::new()
            .name("skimmer".into())
            .spawn(move || skimmer_loop(rx, config, process_ns_t, channels_t, spots_t))
            .expect("spawn skimmer");
        Self {
            tx,
            process_ns,
            channels,
            spots,
            join: Some(join),
        }
    }

    fn submit(
        &self,
        iq: &[Complex32],
        spectrum: &[f32],
        iq_rate: f32,
        spectrum_rate: f32,
        center_hz: f64,
    ) -> bool {
        self.tx
            .try_send(SkimmerMsg::Frame(SkimmerInput {
                iq: iq.to_vec(),
                spectrum: spectrum.to_vec(),
                iq_rate,
                spectrum_rate,
                center_hz,
            }))
            .is_ok()
    }

    fn process_ns(&self) -> u64 {
        self.process_ns.load(Ordering::Relaxed)
    }

    fn channels(&self) -> u64 {
        self.channels.load(Ordering::Relaxed)
    }

    fn spots(&self) -> u64 {
        self.spots.load(Ordering::Relaxed)
    }
}

impl Drop for SkimmerWorker {
    fn drop(&mut self) {
        drop(self.tx.clone());
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

fn skimmer_loop(
    rx: Receiver<SkimmerMsg>,
    config: SkimmerConfig,
    process_ns: Arc<AtomicU64>,
    channels: Arc<AtomicU64>,
    spots: Arc<AtomicU64>,
) {
    let mut sk = Skimmer::new(config);
    while let Ok(SkimmerMsg::Frame(mut input)) = rx.recv() {
        loop {
            match rx.try_recv() {
                Ok(SkimmerMsg::Frame(next)) => input = next,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
        let t0 = Instant::now();
        sk.process(
            &input.iq,
            input.iq_rate,
            &input.spectrum,
            input.spectrum_rate,
            0.0,
            input.center_hz,
        );
        process_ns.fetch_add(t0.elapsed().as_nanos() as u64, Ordering::Relaxed);
        channels.store(sk.active_channels() as u64, Ordering::Relaxed);
        spots.store(sk.store().len() as u64, Ordering::Relaxed);
    }
}

struct CpuSampler {
    stop: Arc<AtomicBool>,
    peak_pct: Arc<AtomicU64>,
    avg_sum: Arc<AtomicU64>,
    samples: Arc<AtomicU64>,
    join: Option<thread::JoinHandle<()>>,
}

impl CpuSampler {
    fn start() -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let peak_pct = Arc::new(AtomicU64::new(0));
        let avg_sum = Arc::new(AtomicU64::new(0));
        let samples = Arc::new(AtomicU64::new(0));
        let stop_t = Arc::clone(&stop);
        let peak_t = Arc::clone(&peak_pct);
        let sum_t = Arc::clone(&avg_sum);
        let n_t = Arc::clone(&samples);
        let join = thread::spawn(move || cpu_sampler_loop(stop_t, peak_t, sum_t, n_t));
        Self {
            stop,
            peak_pct,
            avg_sum,
            samples,
            join: Some(join),
        }
    }

    fn stop(&mut self) -> (f64, f64) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
        let n = self.samples.load(Ordering::Relaxed).max(1) as f64;
        let avg = self.avg_sum.load(Ordering::Relaxed) as f64 / n / 100.0;
        let peak = self.peak_pct.load(Ordering::Relaxed) as f64 / 100.0;
        (avg, peak)
    }
}

fn cpu_sampler_loop(
    stop: Arc<AtomicBool>,
    peak: Arc<AtomicU64>,
    sum: Arc<AtomicU64>,
    samples: Arc<AtomicU64>,
) {
    let mut last_total: u64 = 0;
    let mut last_idle: u64 = 0;
    while !stop.load(Ordering::Relaxed) {
        if let Some((total, idle)) = read_cpu_jiffies() {
            if last_total > 0 && total > last_total {
                let dt = total - last_total;
                let di = idle - last_idle;
                let busy_pct = ((dt - di) as f64 / dt as f64 * 10_000.0) as u64;
                sum.fetch_add(busy_pct, Ordering::Relaxed);
                samples.fetch_add(1, Ordering::Relaxed);
                peak.fetch_max(busy_pct, Ordering::Relaxed);
            }
            last_total = total;
            last_idle = idle;
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn read_cpu_jiffies() -> Option<(u64, u64)> {
    let stat = std::fs::read_to_string("/proc/stat").ok()?;
    let line = stat.lines().next()?;
    let parts: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .filter_map(|s| s.parse().ok())
        .collect();
    if parts.len() < 4 {
        return None;
    }
    let idle = parts[3] + parts.get(4).copied().unwrap_or(0);
    let total: u64 = parts.iter().sum();
    Some((total, idle))
}

fn connect_kiwi(
    host: &str,
    port: u16,
    center_hz: f64,
    opts: &KiwiOpts,
) -> Result<(KiwiSource, hfsdr::Consumer<Complex32>, f32, f32, usize), String> {
    let half = if opts.iq_half_bw_hz == 0 {
        kiwi_iq_half_hz(opts.iq_rate_hz.max(1_000))
    } else {
        opts.iq_half_bw_hz as i32
    };
    let mut src = KiwiSource::new(host, port)
        .with_passband(-half, half)
        .with_ar_out_hz(96_000);
    src.tune(center_hz).map_err(|e| e.to_string())?;
    let device_rate = src.sample_rate() as f32;
    let iq = src.start().map_err(|e| e.to_string())?;
    Ok((src, iq, device_rate, device_rate, 1))
}

fn wait_iq(iq: &mut hfsdr::Consumer<Complex32>, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if iq.pop().is_ok() {
            return true;
        }
        thread::sleep(Duration::from_millis(20));
    }
    false
}

fn main() {
    let host = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "192.36.155.252".into());
    let port: u16 = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(8073);
    let center_hz: f64 = std::env::args()
        .nth(3)
        .and_then(|s| s.parse().ok())
        .unwrap_or(14_010_000.0);
    let run_secs: f64 = std::env::args()
        .nth(4)
        .and_then(|s| s.parse().ok())
        .unwrap_or(15.0);

    let opts = KiwiOpts::default();
    eprintln!(
        "Kiwi pipeline bench: {host}:{port} @ {center_hz:.0} Hz, passband ±{} Hz, skimmer ON",
        opts.iq_half_bw_hz
    );

    let (mut radio, mut iq, device_rate, spectrum_rate, ingress_decim) =
        match connect_kiwi(&host, port, center_hz, &opts) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("connect failed: {e}");
                std::process::exit(1);
            }
        };

    if !wait_iq(&mut iq, Duration::from_secs(45)) {
        eprintln!("Kiwi handshake timed out (no IQ)");
        std::process::exit(1);
    }
    while iq.pop().is_ok() {}

    let (spec_decim, fft_size, eff_rate) =
        spectrum_plan(spectrum_rate, FFT_SIZE, true, spectrum_rate);
    let hop = spectrum_hop(fft_size, spectrum_rate);
    eprintln!(
        "link: device_rate={device_rate} spectrum_rate={spectrum_rate} ingress_decim={ingress_decim} \
         fft={fft_size} hop={hop} spec_decim={spec_decim} eff={eff_rate}"
    );

    let skimmer_config = SkimmerConfig {
        max_channels: 24,
        min_snr_db: 8.7,
        min_decode_snr_db: 18.0,
        decode_gate_ms: 64.0,
        decoder: SkimmerDecoderKind::Bigram,
        require_scp: false,
        source_label: "bench".into(),
        ..SkimmerConfig::default()
    };
    let skimmer = SkimmerWorker::spawn(skimmer_config);

    let mut drain = Vec::with_capacity(MAX_DRAIN);
    let mut drain_decim = Vec::with_capacity(1 << 15);
    let mut spectrum_scratch = Vec::new();
    let mut audio_scratch = Vec::new();
    let mut demod = IqAudioDemod::new();
    let cw = CwChannelSettings::default();
    let mut spectrum_ingress = FirDecimator::with_factor(device_rate, ingress_decim, true);
    let mut spectrum_front = SpectrumFrontEnd::new(spectrum_rate, spec_decim, 0.0);
    let mut analyzer = SpectrumAnalyzer::new(fft_size, hop);
    let mut latest = vec![-120.0; fft_size];

    let warmup = Instant::now();
    while warmup.elapsed().as_secs_f64() < WARMUP_SECS {
        drain.clear();
        while drain.len() < MAX_DRAIN {
            match iq.pop() {
                Ok(s) => drain.push(s),
                Err(_) => break,
            }
        }
        if drain.is_empty() {
            thread::sleep(Duration::from_millis(2));
        }
    }

    let mut cpu = CpuSampler::start();
    let mut t = Timers::default();
    let start = Instant::now();
    let mut skimmer_drops = 0u64;

    while start.elapsed().as_secs_f64() < run_secs {
        let t_drain = Instant::now();
        drain.clear();
        while drain.len() < MAX_DRAIN {
            match iq.pop() {
                Ok(s) => drain.push(s),
                Err(_) => break,
            }
        }
        t.drain_ns += t_drain.elapsed().as_nanos() as u64;
        let got = drain.len();
        if got == 0 {
            thread::sleep(Duration::from_millis(2));
            continue;
        }
        t.samples += got as u64;
        t.pumps += 1;

        let t_demod = Instant::now();
        if ingress_decim > 1 {
            spectrum_ingress.decimate_block(&drain, &mut drain_decim, false);
        }
        demod.process(&drain, device_rate, &cw, &mut audio_scratch);
        t.demod_ns += t_demod.elapsed().as_nanos() as u64;

        let t_ingress = Instant::now();
        let ingress_base: &[Complex32] = if ingress_decim > 1 {
            &drain_decim
        } else {
            &drain
        };
        t.ingress_ns += t_ingress.elapsed().as_nanos() as u64;

        let t_front = Instant::now();
        if spec_decim > 1 {
            spectrum_front.process(ingress_base, &mut spectrum_scratch);
        } else {
            spectrum_scratch.clear();
            spectrum_scratch.extend_from_slice(ingress_base);
        }
        t.spectrum_front_ns += t_front.elapsed().as_nanos() as u64;

        let t_fft = Instant::now();
        let max_rows = 4usize;
        let rows = analyzer.process_limited(&spectrum_scratch, max_rows, |row| {
            latest.copy_from_slice(row);
        });
        t.fft_ns += t_fft.elapsed().as_nanos() as u64;
        t.fft_rows += rows as u64;

        let t_sk = Instant::now();
        let (sk_iq, sk_rate) = if ingress_decim > 1 && !drain_decim.is_empty() {
            (drain_decim.as_slice(), spectrum_rate)
        } else {
            (drain.as_slice(), device_rate)
        };
        if !skimmer.submit(sk_iq, &latest, sk_rate, eff_rate, center_hz) {
            skimmer_drops += 1;
        }
        t.skimmer_submit_ns += t_sk.elapsed().as_nanos() as u64;
    }

    let elapsed = start.elapsed().as_secs_f64();
    let (cpu_avg, cpu_peak) = cpu.stop();
    radio.stop().ok();

    let skimmer_process_ns = skimmer.process_ns();
    let measured_ns = t.drain_ns
        + t.demod_ns
        + t.ingress_ns
        + t.spectrum_front_ns
        + t.fft_ns
        + t.skimmer_submit_ns;
    let engine_ns = measured_ns;
    let pipeline_ns = engine_ns + skimmer_process_ns;

    let sps = t.samples as f64 / elapsed;
    eprintln!("\n=== Kiwi 20m pipeline ({elapsed:.1}s, {} pumps) ===", t.pumps);
    eprintln!(
        "throughput: {sps:.0} IQ samples/s ({:.1}% of {device_rate})",
        sps / device_rate as f64 * 100.0
    );
    eprintln!("fft rows: {}", t.fft_rows);
    eprintln!(
        "skimmer: {} active ch, {} spots, {} submit drops",
        skimmer.channels(),
        skimmer.spots(),
        skimmer_drops
    );
    eprintln!("system CPU: avg {cpu_avg:.1}% peak {cpu_peak:.1}% (all cores)");

    eprintln!("\n--- engine thread (measured) ---");
    for (name, ns) in [
        ("drain", t.drain_ns),
        ("demod+audio", t.demod_ns),
        ("ingress select", t.ingress_ns),
        ("spectrum front", t.spectrum_front_ns),
        ("fft", t.fft_ns),
        ("skimmer submit", t.skimmer_submit_ns),
    ] {
        let pct = ns as f64 / engine_ns as f64 * 100.0;
        let per_pump = ns as f64 / t.pumps as f64 / 1000.0;
        let core_pct = ns as f64 / 1e9 / elapsed * 100.0;
        let ms_per_s = ns as f64 / 1e6 / elapsed;
        eprintln!("  {name:16} {pct:5.1}%  {per_pump:7.1} µs/pump  {ms_per_s:6.1} ms/s  ({core_pct:.1}% core)");
    }
    eprintln!(
        "  engine total       {:.1} ms/s ({:.1}% of 1 core)",
        engine_ns as f64 / 1e6 / elapsed,
        engine_ns as f64 / 1e9 / elapsed * 100.0
    );

    eprintln!("\n--- skimmer thread (measured) ---");
    let sk_pct = skimmer_process_ns as f64 / pipeline_ns as f64 * 100.0;
    eprintln!(
        "  skimmer process  {sk_pct:5.1}%  {:.1} ms/s ({:.1}% of 1 core)",
        skimmer_process_ns as f64 / 1e6 / elapsed,
        skimmer_process_ns as f64 / 1e9 / elapsed * 100.0
    );

    eprintln!("\n--- combined DSP estimate (engine + skimmer) ---");
    eprintln!(
        "  total measured   {:.1} ms/s ({:.1}% of 1 core, {:.1}% est. on {} cores)",
        pipeline_ns as f64 / 1e6 / elapsed,
        pipeline_ns as f64 / 1e9 / elapsed * 100.0,
        pipeline_ns as f64 / 1e9 / elapsed * 100.0 / num_cpus() as f64,
        num_cpus()
    );

    let mut ranked: Vec<(&str, u64)> = vec![
        ("demod+audio", t.demod_ns),
        ("fft", t.fft_ns),
        ("skimmer", skimmer_process_ns),
        ("spectrum front", t.spectrum_front_ns),
        ("drain", t.drain_ns),
        ("skimmer submit", t.skimmer_submit_ns),
        ("ingress", t.ingress_ns),
    ];
    ranked.sort_by(|a, b| b.1.cmp(&a.1));
    eprintln!("\n--- biggest CPU consumers (ranked) ---");
    for (i, (name, ns)) in ranked.iter().enumerate() {
        let pct = *ns as f64 / pipeline_ns as f64 * 100.0;
        eprintln!("  {}. {name}: {pct:.1}%", i + 1);
    }
}

fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}
