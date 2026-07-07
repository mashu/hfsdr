//! Headless skimmer profiler — synthetic multi-channel CW at wideband rates.
//!
//! Usage: `cargo run --release --bin skimmer-bench [seconds]`

use std::f32::consts::TAU;
use std::time::Instant;

use hfsdr::{
    detect_peaks_with_floor, noise_floor_db_into, Complex32, Skimmer, SkimmerConfig,
    SkimmerDecoderKind,
};

const IQ_RATE: f32 = 384_000.0;
const CENTER_HZ: f64 = 14_035_000.0;
const FFT_SIZE: usize = 2048;

#[derive(Default, Clone, Copy)]
struct Timers {
    peaks_ns: u64,
    channels_ns: u64,
    full_ns: u64,
    frames: u64,
}

fn keyed_tone(
    len: usize,
    offset_hz: f32,
    rate: f32,
    wpm: f32,
    text: &str,
    on: bool,
) -> Vec<Complex32> {
    let dot = (1.2 / wpm * rate) as usize;
    let mut out = vec![Complex32 { re: 0.0, im: 0.0 }; len];
    if !on {
        return out;
    }
    let mut phase = 0.0f32;
    let mut pos = 0usize;
    let push = |on: bool, n: usize, phase: &mut f32, out: &mut [Complex32], pos: &mut usize| {
        for _ in 0..n {
            if *pos >= out.len() {
                return;
            }
            *phase += TAU * offset_hz / rate;
            let (s, c) = phase.sin_cos();
            let amp = if on { 0.04 } else { 0.0 };
            out[*pos] = Complex32 {
                re: amp * c,
                im: amp * s,
            };
            *pos += 1;
        }
    };
    push(false, dot * 4, &mut phase, &mut out, &mut pos);
    for (ci, ch) in text.chars().enumerate() {
        if ci > 0 {
            push(false, dot * 3, &mut phase, &mut out, &mut pos);
        }
        let morse: &str = match ch {
            'C' => "-.-.",
            'Q' => "--.-",
            'K' => "-.-",
            '5' => ".....",
            _ => ".",
        };
        for (ei, el) in morse.chars().enumerate() {
            if ei > 0 {
                push(false, dot, &mut phase, &mut out, &mut pos);
            }
            let n = if el == '-' { dot * 3 } else { dot };
            push(true, n, &mut phase, &mut out, &mut pos);
        }
    }
    while pos < out.len() {
        push(false, 1, &mut phase, &mut out, &mut pos);
    }
    out
}

fn mix_channels(offsets: &[(f32, &str)], chunk_len: usize) -> (Vec<Complex32>, Vec<f32>) {
    let mut iq = vec![Complex32 { re: 0.0, im: 0.0 }; chunk_len];
    let mut spectrum = vec![-95.0f32; FFT_SIZE];
    for &(off, text) in offsets {
        let tone = keyed_tone(chunk_len, off, IQ_RATE, 22.0, text, true);
        for (a, b) in iq.iter_mut().zip(tone.iter()) {
            a.re += b.re;
            a.im += b.im;
        }
        let bin = ((off / IQ_RATE) * FFT_SIZE as f32 + FFT_SIZE as f32 / 2.0).round() as usize;
        if bin < FFT_SIZE {
            spectrum[bin] = -28.0;
        }
    }
    (iq, spectrum)
}

fn main() {
    let run_secs: f64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(3.0);

    let offsets: Vec<(f32, &str)> = vec![
        (-2_500.0, "CQ"),
        (-800.0, "CQ"),
        (400.0, "CQ"),
        (1_200.0, "CQ"),
        (2_800.0, "CQ"),
        (-4_000.0, "CQ"),
        (4_500.0, "CQ"),
        (0.0, "CQ"),
    ];

    let chunk_len = (IQ_RATE as usize / 20).max(4096);
    let (iq, spectrum) = mix_channels(&offsets, chunk_len);

    let mut config = SkimmerConfig {
        max_channels: 16,
        min_snr_db: 8.0,
        min_decode_snr_db: 6.0,
        decode_gate_ms: 40.0,
        require_scp: false,
        // Stress the full band; the app default focuses ±1.5 kHz around the
        // tuned frequency, which costs proportionally less.
        focus_span_hz: 0.0,
        ..SkimmerConfig::default()
    };

    eprintln!(
        "skimmer bench: {IQ_RATE} Hz IQ, {} samples/chunk, {} offsets, fft={FFT_SIZE}",
        chunk_len,
        offsets.len()
    );

    let mut sk = Skimmer::new(config.clone());
    for _ in 0..8 {
        sk.process(&iq, IQ_RATE, &spectrum, IQ_RATE, 0.0, CENTER_HZ);
    }

    let mut t = Timers::default();
    let mut floor_scratch = Vec::with_capacity(FFT_SIZE);
    let start = Instant::now();
    while start.elapsed().as_secs_f64() < run_secs {
        let t0 = Instant::now();
        let floor = noise_floor_db_into(&spectrum, &mut floor_scratch);
        let _peaks = detect_peaks_with_floor(&spectrum, IQ_RATE, 8.0, 5, floor);
        t.peaks_ns += t0.elapsed().as_nanos() as u64;

        let t1 = Instant::now();
        sk.process(&iq, IQ_RATE, &spectrum, IQ_RATE, 0.0, CENTER_HZ);
        t.full_ns += t1.elapsed().as_nanos() as u64;
        t.channels_ns += t1.elapsed().as_nanos() as u64;
        t.frames += 1;
    }
    let elapsed = start.elapsed().as_secs_f64();

    // Decoder comparison microbench (same IQ, isolated).
    config.decoder = SkimmerDecoderKind::Adaptive;
    let mut sk_adaptive = Skimmer::new(config.clone());
    config.decoder = SkimmerDecoderKind::Bigram;
    let mut sk_bigram = Skimmer::new(config);

    let warmup = 4usize;
    for _ in 0..warmup {
        sk_adaptive.process(&iq, IQ_RATE, &spectrum, IQ_RATE, 0.0, CENTER_HZ);
        sk_bigram.process(&iq, IQ_RATE, &spectrum, IQ_RATE, 0.0, CENTER_HZ);
    }
    let n = 200u32;
    let t0 = Instant::now();
    for _ in 0..n {
        sk_adaptive.process(&iq, IQ_RATE, &spectrum, IQ_RATE, 0.0, CENTER_HZ);
    }
    let adaptive_us = t0.elapsed().as_nanos() as f64 / n as f64 / 1000.0;
    let t1 = Instant::now();
    for _ in 0..n {
        sk_bigram.process(&iq, IQ_RATE, &spectrum, IQ_RATE, 0.0, CENTER_HZ);
    }
    let bigram_us = t1.elapsed().as_nanos() as f64 / n as f64 / 1000.0;

    let sps = t.frames as f64 * chunk_len as f64 / elapsed;
    eprintln!("\n=== skimmer bench ({elapsed:.2}s, {} frames) ===", t.frames);
    eprintln!(
        "throughput: {sps:.0} IQ samples/s ({:.1}% of {IQ_RATE})",
        sps / IQ_RATE as f64 * 100.0
    );
    eprintln!("active channels: {}", sk.active_channels());
    eprintln!("spots: {}", sk.store().len());
    eprintln!(
        "  peaks only       {:8.1} µs/frame",
        t.peaks_ns as f64 / t.frames as f64 / 1000.0
    );
    eprintln!(
        "  full skimmer     {:8.1} µs/frame",
        t.full_ns as f64 / t.frames as f64 / 1000.0
    );
    eprintln!(
        "  chunk_len        {} samples ({:.2} ms @ {IQ_RATE} Hz)",
        chunk_len,
        chunk_len as f64 / IQ_RATE as f64 * 1000.0
    );
    eprintln!("\nfixed chunk decoder ({} ch, {} samples):", offsets.len(), chunk_len);
    eprintln!("  adaptive         {adaptive_us:.1} µs/frame");
    eprintln!("  bigram           {bigram_us:.1} µs/frame");
}
