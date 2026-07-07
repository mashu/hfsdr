//! Empirical check: block (FFT) FIR path vs scalar path, sample-exact alignment.

use hfsdr::source::Complex32;
use hfsdr::dsp::{design_lowpass_with, FirFilter, LowpassDesign};
use hfsdr::WindowKind;

fn tone(rate: f32, hz: f32, n: usize) -> Vec<Complex32> {
    (0..n)
        .map(|i| {
            let t = i as f32 / rate;
            let p = std::f32::consts::TAU * hz * t;
            Complex32::new(p.cos(), p.sin())
        })
        .collect()
}

#[test]
fn block_fft_path_is_sample_aligned_with_scalar() {
    let rate = 12_000.0;
    // Force a long filter so the FFT block path actually triggers (>= 64 taps).
    let proto = design_lowpass_with(
        rate,
        200.0,
        LowpassDesign {
            window: WindowKind::Blackman,
            deep_selectivity: true,
            ..LowpassDesign::default()
        },
    );
    assert!(proto.len() >= 64, "need >=64 taps to hit FFT path, got {}", proto.len());

    let input = tone(rate, 150.0, 4096);

    let mut scalar = FirFilter::new(proto.taps().to_vec());
    let scalar_out: Vec<Complex32> = input.iter().map(|&s| scalar.process_complex(s)).collect();

    let mut block = FirFilter::new(proto.taps().to_vec());
    let mut block_out = Vec::new();
    // Feed in two chunks to also exercise the cross-block state handoff.
    let mut part = Vec::new();
    block.process_complex_block(&input[..2048], &mut part);
    block_out.extend_from_slice(&part);
    block.process_complex_block(&input[2048..], &mut part);
    block_out.extend_from_slice(&part);

    assert_eq!(scalar_out.len(), block_out.len());
    let mut max_err = 0.0f32;
    let mut max_i = 0usize;
    for (i, (a, b)) in scalar_out.iter().zip(block_out.iter()).enumerate() {
        let e = (a.re - b.re).abs() + (a.im - b.im).abs();
        if e > max_err {
            max_err = e;
            max_i = i;
        }
    }
    // Also measure error against a one-sample-shifted comparison to diagnose off-by-one.
    let mut shift_err = 0.0f32;
    for i in 1..scalar_out.len() {
        let a = scalar_out[i - 1];
        let b = block_out[i];
        shift_err += (a.re - b.re).abs() + (a.im - b.im).abs();
    }
    let direct_err: f32 = scalar_out
        .iter()
        .zip(block_out.iter())
        .map(|(a, b)| (a.re - b.re).abs() + (a.im - b.im).abs())
        .sum();
    eprintln!(
        "taps={} direct_err={direct_err:.4} shifted_err={shift_err:.4} max_err={max_err:.6} at {max_i}",
        proto.len()
    );
    assert!(
        max_err < 1e-3,
        "block FFT FIR output misaligned vs scalar: max_err={max_err} at sample {max_i} \
         (direct_err={direct_err}, one-sample-shift err={shift_err})"
    );
}
