//! Minimal DSP for the panadapter: a windowed, overlapping complex FFT that
//! turns a stream of baseband IQ into rows of power spectral density (dB),
//! arranged with DC in the centre (fftshift) so the display reads
//! negative frequencies on the left, positive on the right.

use crate::source::Complex32;
use rustfft::{Fft, FftPlanner};
use std::sync::Arc;

/// Streaming spectrum analyzer. Feed it IQ with [`SpectrumAnalyzer::process`];
/// it invokes a closure with each completed FFT row.
pub struct SpectrumAnalyzer {
    fft: Arc<dyn Fft<f32>>,
    n: usize,
    hop: usize,
    window: Vec<f32>,
    coherent_gain: f32,
    acc: Vec<Complex32>,
    buf: Vec<Complex32>,
    scratch: Vec<Complex32>,
    row: Vec<f32>,
}

impl SpectrumAnalyzer {
    /// `n` is the FFT size (and the number of frequency bins / row width).
    /// `hop` is how many new samples advance the window each frame: use `n`
    /// for no overlap, `n / 2` for 50% overlap (smoother waterfall).
    pub fn new(n: usize, hop: usize) -> Self {
        assert!(n.is_power_of_two(), "FFT size should be a power of two");
        let hop = hop.clamp(1, n);
        let fft = FftPlanner::<f32>::new().plan_fft_forward(n);
        let scratch = vec![Complex32::new(0.0, 0.0); fft.get_inplace_scratch_len()];

        // Hann window and its coherent gain (mean) for amplitude normalization.
        let window: Vec<f32> = (0..n)
            .map(|i| {
                let x = std::f32::consts::PI * i as f32 / (n as f32 - 1.0);
                x.sin().powi(2)
            })
            .collect();
        let coherent_gain = window.iter().sum::<f32>() / n as f32;

        Self {
            fft,
            n,
            hop,
            window,
            coherent_gain,
            acc: Vec::with_capacity(n),
            buf: vec![Complex32::new(0.0, 0.0); n],
            scratch,
            row: vec![0.0; n],
        }
    }

    /// FFT size / row width in bins.
    pub fn size(&self) -> usize {
        self.n
    }

    /// Feed IQ samples; `emit` is called once per completed row with a slice of
    /// `size()` dB values (fftshifted: index 0 is the lowest frequency).
    pub fn process<F: FnMut(&[f32])>(&mut self, input: &[Complex32], mut emit: F) {
        for &s in input {
            self.acc.push(s);
            if self.acc.len() < self.n {
                continue;
            }
            // Window into the FFT buffer.
            for i in 0..self.n {
                self.buf[i] = self.acc[i] * self.window[i];
            }
            self.fft.process_with_scratch(&mut self.buf, &mut self.scratch);

            // Magnitude -> dB, with fftshift so DC lands in the middle.
            let half = self.n / 2;
            let norm = self.n as f32 * self.coherent_gain;
            for i in 0..self.n {
                let src = (i + half) % self.n;
                let mag = self.buf[src].norm() / norm;
                self.row[i] = 20.0 * (mag + 1e-12).log10();
            }
            emit(&self.row);

            // Advance by hop, keeping the overlap tail.
            self.acc.drain(0..self.hop);
        }
    }
}
