//! Offline skimmer decode of a recorded `.hiq.gz` capture.
//!
//! ```bash
//! CAPTURE=/path/to/capture.hiq.gz cargo test --test decode_capture -- --ignored --nocapture
//! ```

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use flate2::read::GzDecoder;
use hfsdr::{
    detect_peaks, read_meta, strongest_offset_hz, Complex32, DecoderParams, EnvelopeSettings,
    MasterScp, Skimmer, SkimmerConfig, SpectrumAnalyzer, SpotSort,
};

fn load_capture(path: &Path) -> (hfsdr::IqCaptureMeta, Vec<Complex32>) {
    let meta = read_meta(path).expect("read meta");
    let mut file = File::open(path).expect("open");
    file.seek(SeekFrom::Start(32)).expect("seek");
    let mut dec = GzDecoder::new(file);
    let mut raw = Vec::new();
    dec.read_to_end(&mut raw).expect("decompress");
    let mut samples = Vec::with_capacity(raw.len() / 8);
    for chunk in raw.chunks_exact(8) {
        let re = f32::from_le_bytes(chunk[0..4].try_into().unwrap());
        let im = f32::from_le_bytes(chunk[4..8].try_into().unwrap());
        samples.push(Complex32 { re, im });
    }
    (meta, samples)
}

fn spectrum_with_peak(offset: f32, rate: f32, n: usize, snr_db: f32) -> Vec<f32> {
    let mut row = vec![-100.0f32; n];
    let floor = -100.0;
    let bin = ((offset / rate) * n as f32 + n as f32 / 2.0).round() as usize;
    if bin < n {
        row[bin] = floor + snr_db;
    }
    row
}

fn run_skimmer(
    samples: &[Complex32],
    rate: f32,
    center: f64,
    spectrum: &[f32],
    label: &str,
    config: SkimmerConfig,
) -> (Skimmer, String) {
    let mut skimmer = Skimmer::new(config);
    skimmer.reload_scp_discover();
    let chunk_size = 2048usize;
    for chunk in samples.chunks(chunk_size) {
        skimmer.process(chunk, rate, spectrum, rate, 0.0, center);
    }
    let spots = skimmer
        .store()
        .sorted(SpotSort::SnrDesc)
        .into_iter()
        .map(|s| {
            format!(
                "{:.1}kHz SNR{:.0} {:?} {}",
                s.frequency_hz / 1e3,
                s.snr_db,
                s.kind,
                s.callsign.as_deref().unwrap_or("—")
            )
        })
        .collect::<Vec<_>>()
        .join("\n  ");
    (skimmer, format!("{label}:\n  {spots}"))
}

#[test]
#[ignore]
fn decode_capture_file() {
    let path = std::env::var("CAPTURE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::config_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join("hfsdr")
                .join("capture-1782200354.hiq.gz")
        });
    let (meta, samples) = load_capture(&path);
    eprintln!(
        "capture: {} samples @ {} Hz, center {:.6} MHz, {:.1}s",
        samples.len(),
        meta.sample_rate,
        meta.center_hz / 1e6,
        samples.len() as f64 / meta.sample_rate as f64
    );
    assert!(
        samples.len() > 10_000,
        "expected substantial IQ data in {}",
        path.display()
    );

    let scp = MasterScp::discover();
    eprintln!(
        "MASTER.SCP: {}",
        if scp.is_loaded() {
            format!("{} calls", scp.len())
        } else {
            "not loaded".into()
        }
    );

    let rate = meta.sample_rate as f32;
    let center = meta.center_hz;
    let mut analyzer = SpectrumAnalyzer::new(2048, 1024);
    let mut peak_row = vec![-120.0f32; 2048];
    analyzer.process(&samples, |row| {
        for (d, s) in peak_row.iter_mut().zip(row.iter()) {
            *d = d.max(*s);
        }
    });
    let peaks = detect_peaks(&peak_row, rate, 7.0, 5);
    eprintln!("detected peaks (max-hold spectrum):");
    for p in peaks.iter().take(15) {
        eprintln!("  {:7.1} Hz  SNR {:.0} dB", p.offset_hz, p.snr_db);
    }
    let peak = strongest_offset_hz(&peak_row, rate, 0.0, 600.0).expect("no peak in capture");
    let peaks = detect_peaks(&peak_row, rate, 0.0, 1);
    let snr = peaks
        .iter()
        .find(|p| (p.offset_hz - peak).abs() < 5.0)
        .map(|p| p.snr_db)
        .unwrap_or(20.0);
    eprintln!(
        "strongest peak: {:.1} Hz offset ({:.3} kHz abs), {:.0} dB SNR",
        peak,
        (center + peak as f64) / 1e3,
        snr
    );

    let base = SkimmerConfig {
        min_snr_db: 7.0,
        min_decode_snr_db: 5.0,
        decode_gate_ms: 45.0,
        decoder_params: DecoderParams {
            initial_wpm: 22.0,
            envelope: EnvelopeSettings {
                thr_low: 0.48,
                thr_high: 0.65,
                min_span_fraction: 0.06,
            },
            ..DecoderParams::default()
        },
        ..SkimmerConfig::default()
    };

    let live_spectrum = peak_row.clone();
    let (_, out_live) = run_skimmer(
        &samples,
        rate,
        center,
        &live_spectrum,
        "live spectrum (all peaks)",
        base.clone(),
    );
    eprintln!("\n{out_live}");

    let single_peak = spectrum_with_peak(peak, rate, 2048, snr.max(15.0));
    let (_, out_single) = run_skimmer(
        &samples,
        rate,
        center,
        &single_peak,
        "single peak at strongest",
        SkimmerConfig {
            require_scp: false,
            min_decode_snr_db: 0.0,
            decode_gate_ms: 30.0,
            max_channels: 1,
            ..base.clone()
        },
    );
    eprintln!("\n{out_single}");
    let mut sk_one = Skimmer::new(SkimmerConfig {
        require_scp: false,
        min_decode_snr_db: 0.0,
        decode_gate_ms: 30.0,
        max_channels: 1,
        ..base.clone()
    });
    for chunk in samples.chunks(2048) {
        sk_one.process(chunk, rate, &single_peak, rate, 0.0, center);
    }
    for (off, text, snr) in sk_one.debug_channels() {
        eprintln!("single-peak channel text @ {off:.0} Hz SNR {snr:.0}: {text:?}");
    }

    for wpm in [18.0, 22.0, 28.0, 35.0, 45.0, 60.0] {
        let (_, out) = run_skimmer(
            &samples,
            rate,
            center,
            &single_peak,
            &format!("wpm hint {wpm}"),
            SkimmerConfig {
                require_scp: false,
                min_decode_snr_db: 0.0,
                decode_gate_ms: 30.0,
                max_channels: 1,
                decoder_params: DecoderParams {
                    initial_wpm: wpm,
                    ..base.decoder_params
                },
                ..base.clone()
            },
        );
        if out.contains("CQ") {
            eprintln!("\n{out}");
        }
    }

    // Raw decoder text via relaxed SCP (shows CQ even without a callsign).
    let (_, out_relaxed) = run_skimmer(
        &samples,
        rate,
        center,
        &single_peak,
        "relaxed (no SCP required)",
        SkimmerConfig {
            require_scp: false,
            min_decode_snr_db: 0.0,
            decode_gate_ms: 30.0,
            max_channels: 1,
            ..base
        },
    );
    eprintln!("\n{out_relaxed}");

    // Dump raw channel text from the default skimmer config.
    let mut skimmer = Skimmer::new(SkimmerConfig {
        require_scp: false,
        min_decode_snr_db: 0.0,
        decode_gate_ms: 20.0,
        max_channels: 4,
        ..SkimmerConfig::default()
    });
    skimmer.reload_scp_discover();
    for chunk in samples.chunks(2048) {
        analyzer.process(chunk, |row| peak_row.copy_from_slice(row));
        skimmer.process(chunk, rate, &peak_row, rate, 0.0, center);
    }
    eprintln!("\nchannel debug:");
    for (off, text, snr) in skimmer.debug_channels() {
        eprintln!("  offset {off:.0} Hz SNR {snr:.0}: {text:?}");
    }
}

/// Whole-band replay: decode every peak in the capture (focus off) and dump
/// spots plus per-channel text. `CAPTURE=path cargo test --release --test
/// decode_capture replay_whole_band -- --ignored --nocapture`
#[test]
#[ignore]
fn replay_whole_band() {
    let path = std::path::PathBuf::from(std::env::var("CAPTURE").expect("CAPTURE env"));
    let (meta, samples) = load_capture(&path);
    let rate = meta.sample_rate as f32;
    let center = meta.center_hz;
    eprintln!(
        "capture: {} samples @ {} Hz, center {:.6} MHz, {:.1}s",
        samples.len(),
        meta.sample_rate,
        center / 1e6,
        samples.len() as f64 / rate as f64
    );
    let mut analyzer = SpectrumAnalyzer::new(2048, 1024);
    let mut peak_hold = vec![-140.0f32; 2048];
    let mut skimmer = Skimmer::new(SkimmerConfig {
        require_scp: false,
        min_snr_db: 10.0,
        min_decode_snr_db: 8.0,
        focus_span_hz: 0.0,
        max_channels: 12,
        ..SkimmerConfig::default()
    });
    skimmer.reload_scp_discover();
    for chunk in samples.chunks(2048) {
        analyzer.process_limited(chunk, 1, |row| {
            for (hold, &v) in peak_hold.iter_mut().zip(row.iter()) {
                *hold = hold.max(v);
            }
        });
        skimmer.process(chunk, rate, &peak_hold, rate, 0.0, center);
    }
    let spots = skimmer.store().sorted(SpotSort::SnrDesc);
    eprintln!("spots: {}", spots.len());
    for s in &spots {
        eprintln!(
            "  {:.2} kHz SNR{:.0} {:?} {}",
            s.frequency_hz / 1e3,
            s.snr_db,
            s.kind,
            s.callsign.as_deref().unwrap_or("—")
        );
    }
    eprintln!("channels:");
    for (off, text, snr) in skimmer.debug_channels() {
        eprintln!("  {off:7.0} Hz SNR {snr:4.0}: {text:?}");
    }
}

/// Mirrors the engine path: small IQ chunks + peak-hold spectrum for skimmer peaks.
#[test]
#[ignore]
fn decode_capture_engine_style() {
    let path = std::env::var("CAPTURE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::config_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join("hfsdr")
                .join("capture-1782204072.hiq.gz")
        });
    let (meta, samples) = load_capture(&path);
    let rate = meta.sample_rate as f32;
    let center = meta.center_hz;
    let mut analyzer = SpectrumAnalyzer::new(2048, 1024);
    let mut peak_hold = vec![-120.0f32; 2048];
    let mut skimmer = Skimmer::new(SkimmerConfig::default());
    skimmer.reload_scp_discover();
    let chunk_size = 400usize;
    for chunk in samples.chunks(chunk_size) {
        analyzer.process_limited(chunk, 1, |row| {
            for (hold, &sample) in peak_hold.iter_mut().zip(row.iter()) {
                *hold = hold.max(sample);
            }
        });
        skimmer.process(chunk, rate, &peak_hold, rate, 0.0, center);
    }
    let spots = skimmer.store().sorted(SpotSort::SnrDesc);
    eprintln!("engine-style spots: {}", spots.len());
    for s in spots.iter().take(5) {
        eprintln!(
            "  {:.1}kHz SNR{:.0} {:?} {}",
            s.frequency_hz / 1e3,
            s.snr_db,
            s.kind,
            s.callsign.as_deref().unwrap_or("—")
        );
    }
    assert!(!spots.is_empty(), "expected spots in engine-style replay");
}
