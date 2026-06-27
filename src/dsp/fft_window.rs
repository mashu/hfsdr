//! FFT analysis windows for the panadapter / waterfall spectrum path.
//!
//! Separate from [`super::cw::fir::WindowKind`], which shapes CW channel FIR taps.

use std::f32::consts::PI;

use super::cw::{MAX_KAISER_BETA, MIN_KAISER_BETA};

/// Default FFT window — matches the legacy fixed Hann used before this setting existed.
pub const DEFAULT_FFT_WINDOW: FftWindowKind = FftWindowKind::Hann;

/// Window applied before the spectrum FFT.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum FftWindowKind {
    #[default]
    Hann,
    Hamming,
    Blackman,
    Rectangular,
    Kaiser,
    BlackmanHarris,
    Bartlett,
    Flattop,
}

impl FftWindowKind {
    pub const ALL: [Self; 8] = [
        Self::Hann,
        Self::Hamming,
        Self::Blackman,
        Self::Rectangular,
        Self::Kaiser,
        Self::BlackmanHarris,
        Self::Bartlett,
        Self::Flattop,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Hann => "Hann",
            Self::Hamming => "Hamming",
            Self::Blackman => "Blackman",
            Self::Rectangular => "Rectangular",
            Self::Kaiser => "Kaiser",
            Self::BlackmanHarris => "Blackman-Harris",
            Self::Bartlett => "Bartlett",
            Self::Flattop => "Flattop",
        }
    }
}

/// Build a length-`n` analysis window for the given kind.
pub fn build_fft_window(n: usize, kind: FftWindowKind, kaiser_beta: f32) -> Vec<f32> {
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        return vec![1.0];
    }
    let m = (n - 1) as f32;
    let beta = kaiser_beta.clamp(MIN_KAISER_BETA, MAX_KAISER_BETA);
    (0..n)
        .map(|k| fft_window_sample(k as f32, m, kind, beta).max(0.0))
        .collect()
}

fn fft_window_sample(k: f32, m: f32, kind: FftWindowKind, kaiser_beta: f32) -> f32 {
    let phase = 2.0 * PI * k / m;
    match kind {
        FftWindowKind::Hann => 0.5 - 0.5 * phase.cos(),
        FftWindowKind::Hamming => 0.54 - 0.46 * phase.cos(),
        FftWindowKind::Blackman => {
            0.42 - 0.5 * phase.cos() + 0.08 * (2.0 * phase).cos()
        }
        FftWindowKind::Rectangular => 1.0,
        FftWindowKind::Kaiser => kaiser_window(k, m, kaiser_beta),
        FftWindowKind::BlackmanHarris => {
            0.35875
                - 0.48829 * phase.cos()
                + 0.14128 * (2.0 * phase).cos()
                - 0.01168 * (3.0 * phase).cos()
        }
        FftWindowKind::Bartlett => 1.0 - (2.0 * k / m - 1.0).abs(),
        FftWindowKind::Flattop => {
            const A0: f32 = 0.21557895;
            const A1: f32 = 0.41663158;
            const A2: f32 = 0.277263158;
            const A3: f32 = 0.083578947;
            const A4: f32 = 0.006947368;
            A0 - A1 * phase.cos()
                + A2 * (2.0 * phase).cos()
                - A3 * (3.0 * phase).cos()
                + A4 * (4.0 * phase).cos()
        }
    }
}

fn kaiser_window(k: f32, m: f32, beta: f32) -> f32 {
    let x = 2.0 * k / m - 1.0;
    let inner = (1.0 - x * x).max(0.0);
    bessel_i0(beta * inner.sqrt()) / bessel_i0(beta)
}

fn bessel_i0(x: f32) -> f32 {
    let x = x.abs();
    let mut sum = 1.0f32;
    let mut term = 1.0f32;
    for i in 1..24 {
        term *= (x / 2.0).powi(2) / (i as f32).powi(2);
        sum += term;
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::cw::DEFAULT_KAISER_BETA;

    #[test]
    fn default_is_hann() {
        assert_eq!(FftWindowKind::default(), FftWindowKind::Hann);
    }

    #[test]
    fn all_windows_finite_and_non_negative() {
        for kind in FftWindowKind::ALL {
            let w = build_fft_window(256, kind, DEFAULT_KAISER_BETA);
            assert_eq!(w.len(), 256);
            assert!(w.iter().all(|v| v.is_finite() && *v >= 0.0));
        }
    }

    #[test]
    fn hann_matches_legacy_sine_squared() {
        let n = 2048;
        let legacy: Vec<f32> = (0..n)
            .map(|i| {
                let x = PI * i as f32 / (n as f32 - 1.0);
                x.sin().powi(2)
            })
            .collect();
        let hann = build_fft_window(n, FftWindowKind::Hann, DEFAULT_KAISER_BETA);
        for (a, b) in legacy.iter().zip(hann.iter()) {
            assert!((a - b).abs() < 1e-6, "legacy={a} hann={b}");
        }
    }

    #[test]
    fn rectangular_is_flat() {
        let w = build_fft_window(128, FftWindowKind::Rectangular, DEFAULT_KAISER_BETA);
        assert!(w.iter().all(|v| (*v - 1.0).abs() < 1e-6));
    }
}
