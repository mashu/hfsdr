//! Windowed, overlapping complex FFT that turns baseband IQ into fftshifted
//! power spectral density rows (dB).

use crate::source::Complex32;

use super::fft_plan::plan_forward;
use rustfft::Fft;
use std::sync::Arc;

/// Fixed-capacity ring for sliding-window FFT input. Avoids O(n) `Vec::drain`
/// on every hop when sample rates are high.
struct SampleRing {
    data: Vec<Complex32>,
    head: usize,
    count: usize,
    capacity: usize,
}

impl SampleRing {
    fn new(capacity: usize) -> Self {
        Self {
            data: vec![Complex32::new(0.0, 0.0); capacity],
            head: 0,
            count: 0,
            capacity,
        }
    }

    fn push(&mut self, sample: Complex32) {
        if self.count < self.capacity {
            let tail = (self.head + self.count) % self.capacity;
            self.data[tail] = sample;
            self.count += 1;
        } else {
            self.data[self.head] = sample;
            self.head = (self.head + 1) % self.capacity;
        }
    }

    fn sample_at(&self, index: usize) -> Complex32 {
        self.data[(self.head + index) % self.capacity]
    }

    fn advance(&mut self, hop: usize) {
        let hop = hop.min(self.count);
        self.head = (self.head + hop) % self.capacity;
        self.count -= hop;
    }

    fn len(&self) -> usize {
        self.count
    }
}

/// Streaming spectrum analyzer. Feed IQ with [`SpectrumAnalyzer::process`];
/// it invokes a closure with each completed FFT row.
pub struct SpectrumAnalyzer {
    fft: Arc<dyn Fft<f32>>,
    n: usize,
    hop: usize,
    window: Vec<f32>,
    coherent_gain: f32,
    acc: SampleRing,
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
        let fft = plan_forward(n);
        let scratch = vec![Complex32::new(0.0, 0.0); fft.get_inplace_scratch_len()];

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
            acc: SampleRing::new(n),
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
    pub fn process<F: FnMut(&[f32])>(&mut self, input: &[Complex32], emit: F) {
        let _ = self.process_limited(input, usize::MAX, emit);
    }

    /// Like [`process`](Self::process) but emits at most `max_rows` frames per call.
    /// All `input` samples are always ingested into the sliding window; emission
    /// stops once `max_rows` is reached, and remaining frames are produced on later
    /// calls as more IQ arrives.
    pub fn process_limited<F: FnMut(&[f32])>(
        &mut self,
        input: &[Complex32],
        max_rows: usize,
        mut emit: F,
    ) -> usize {
        let mut emitted = 0usize;
        for &sample in input {
            self.acc.push(sample);
            while self.acc.len() >= self.n {
                if emitted >= max_rows {
                    break;
                }
                for i in 0..self.n {
                    let s = self.acc.sample_at(i);
                    let w = self.window[i];
                    self.buf[i] = Complex32 {
                        re: s.re * w,
                        im: s.im * w,
                    };
                }
                self.fft.process_with_scratch(&mut self.buf, &mut self.scratch);

                let half = self.n / 2;
                let norm = self.n as f32 * self.coherent_gain;
                for i in 0..self.n {
                    let src = (i + half) % self.n;
                    let re = self.buf[src].re;
                    let im = self.buf[src].im;
                    let mag = (re * re + im * im).sqrt() / norm;
                    self.row[i] = 20.0 * (mag + 1e-12).log10();
                }
                emit(&self.row);
                emitted += 1;

                self.acc.advance(self.hop);
            }
        }
        emitted
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    #[test]
    fn size_matches_fft_bins() {
        let sa = SpectrumAnalyzer::new(256, 128);
        assert_eq!(sa.size(), 256);
    }

    #[test]
    fn constant_signal_produces_finite_db() {
        let mut sa = SpectrumAnalyzer::new(64, 32);
        let tone = Complex32::new(1.0, 0.0);
        let mut rows = 0usize;
        sa.process(&vec![tone; 128], |_| rows += 1);
        assert!(rows > 0);
    }

    #[test]
    fn tone_peak_near_center_bin() {
        let n = 256;
        let hop = n / 2;
        let mut sa = SpectrumAnalyzer::new(n, hop);

        // Tone at +1 kHz in a 12 kHz span: bin offset ≈ n/12 from DC (center).
        let sr = 12_000.0;
        let freq = 1_000.0;
        let samples: Vec<Complex32> = (0..n * 3)
            .map(|t| {
                let phase = TAU * freq * t as f32 / sr;
                Complex32::new(phase.cos(), phase.sin())
            })
            .collect();

        let mut last_row = vec![0.0; n];
        sa.process(&samples, |row| last_row.copy_from_slice(row));

        let center = n / 2;
        let peak_bin = last_row
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.total_cmp(b))
            .map(|(i, _)| i)
            .expect("row emitted");

        let expected = center + (freq / sr * n as f32).round() as usize;
        assert!(
            (peak_bin as i32 - expected as i32).abs() <= 2,
            "peak at bin {peak_bin}, expected near {expected}"
        );
    }

    #[test]
    fn hop_overlap_emits_multiple_rows() {
        let n = 64;
        let hop = n / 2;
        let mut sa = SpectrumAnalyzer::new(n, hop);
        let mut rows = 0usize;
        sa.process(&vec![Complex32::new(0.5, 0.0); n + hop], |_| rows += 1);
        assert_eq!(rows, 2);
    }

    #[test]
    fn process_limited_caps_rows_and_preserves_tail() {
        let n = 64;
        let hop = n / 2;
        let mut sa = SpectrumAnalyzer::new(n, hop);
        let input = vec![Complex32::new(0.5, 0.0); n + hop * 3];
        let mut rows = 0usize;
        sa.process_limited(&input, 2, |_| rows += 1);
        assert_eq!(rows, 2);
        sa.process_limited(&[], 2, |_| rows += 1);
        assert!(rows >= 2);
    }

    #[test]
    fn process_limited_keeps_ingesting_after_row_cap() {
        let n = 64;
        let hop = n;
        let mut sa = SpectrumAnalyzer::new(n, hop);
        let input = vec![Complex32::new(0.5, 0.0); n * 3];
        let mut rows = 0usize;
        sa.process_limited(&input, 1, |_| rows += 1);
        assert_eq!(rows, 1);
        sa.process_limited(&input, 1, |_| rows += 1);
        assert_eq!(rows, 2);
    }
}
