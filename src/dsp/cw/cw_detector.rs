//! CW demodulator — product, coherent, and dit-matched modes.
//!
//! All modes finish with a BFO product detector for audible pitch. Coherent and
//! matched modes integrate complex baseband over a sliding window first, trading
//! latency for several dB of SNR on very weak carriers.

use crate::source::Complex32;

use super::filter_plan::dit_samples;
use super::nco::ComplexNco;
use super::settings::CwSideband;

/// CW demodulation strategy (after channel filter + AGC).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CwDetectorMode {
    /// Classic BFO product detector (default).
    #[default]
    Product,
    /// Complex moving-average integration before product demod.
    Coherent,
    /// Dit-length matched integration (WPM-driven window).
    MatchedDit,
}

/// BFO demodulator with optional coherent / matched pre-integration.
#[derive(Clone, Debug)]
pub struct CwDetector {
    bfo: ComplexNco,
    mode: CwDetectorMode,
    coherent_buf: Vec<Complex32>,
    coherent_pos: usize,
    coherent_sum: Complex32,
    coherent_len: usize,
    last_coherent_len: usize,
    dit_power_buf: Vec<f32>,
    dit_power_sum: f32,
    dit_pos: usize,
    dit_len: usize,
    last_dit_len: usize,
    last_wpm: f32,
}

impl Default for CwDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl CwDetector {
    pub fn new() -> Self {
        Self {
            bfo: ComplexNco::new(),
            mode: CwDetectorMode::Product,
            coherent_buf: Vec::new(),
            coherent_pos: 0,
            coherent_sum: Complex32::new(0.0, 0.0),
            coherent_len: 1,
            last_coherent_len: 0,
            dit_power_buf: Vec::new(),
            dit_power_sum: 0.0,
            dit_pos: 0,
            dit_len: 1,
            last_dit_len: 0,
            last_wpm: 0.0,
        }
    }

    pub fn reset_state(&mut self) {
        self.bfo.reset();
        self.coherent_buf.fill(Complex32::new(0.0, 0.0));
        self.coherent_pos = 0;
        self.coherent_sum = Complex32::new(0.0, 0.0);
        self.dit_power_buf.fill(0.0);
        self.dit_power_sum = 0.0;
        self.dit_pos = 0;
    }

    fn sync_windows(
        &mut self,
        sample_rate: f32,
        mode: CwDetectorMode,
        wpm: f32,
        matched_dit_samples: Option<usize>,
    ) {
        let coherent_len = match mode {
            CwDetectorMode::Product => 1,
            CwDetectorMode::Coherent => {
                (sample_rate * 0.012).round().max(8.0) as usize
            }
            CwDetectorMode::MatchedDit => matched_dit_samples
                .unwrap_or_else(|| dit_samples(wpm, sample_rate))
                .max(1),
        };
        if coherent_len != self.last_coherent_len {
            self.coherent_buf = vec![Complex32::new(0.0, 0.0); coherent_len.max(1)];
            self.coherent_pos = 0;
            self.coherent_sum = Complex32::new(0.0, 0.0);
            self.coherent_len = coherent_len.max(1);
            self.last_coherent_len = coherent_len;
        }
        let dit_len = if mode == CwDetectorMode::MatchedDit {
            matched_dit_samples
                .unwrap_or_else(|| dit_samples(wpm, sample_rate))
                .max(1)
        } else {
            1
        };
        if dit_len != self.last_dit_len || (mode == CwDetectorMode::MatchedDit && wpm != self.last_wpm)
        {
            self.dit_power_buf = vec![0.0; dit_len.max(1)];
            self.dit_power_sum = 0.0;
            self.dit_pos = 0;
            self.dit_len = dit_len.max(1);
            self.last_dit_len = dit_len;
            self.last_wpm = wpm;
        }
        self.mode = mode;
    }

    fn push_coherent(&mut self, sample: Complex32) -> Complex32 {
        if self.coherent_len <= 1 {
            return sample;
        }
        let old = self.coherent_buf[self.coherent_pos];
        self.coherent_buf[self.coherent_pos] = sample;
        self.coherent_sum = Complex32::new(
            self.coherent_sum.re - old.re + sample.re,
            self.coherent_sum.im - old.im + sample.im,
        );
        self.coherent_pos = (self.coherent_pos + 1) % self.coherent_len;
        let n = self.coherent_len as f32;
        Complex32::new(self.coherent_sum.re / n, self.coherent_sum.im / n)
    }

    fn matched_envelope(&mut self, sample: Complex32) -> f32 {
        if self.dit_len <= 1 {
            return 1.0;
        }
        let p = sample.norm_sqr();
        let old = self.dit_power_buf[self.dit_pos];
        self.dit_power_buf[self.dit_pos] = p;
        self.dit_power_sum += p - old;
        self.dit_pos = (self.dit_pos + 1) % self.dit_len;
        (self.dit_power_sum / self.dit_len as f32).sqrt().max(1e-8)
    }

    fn product_demod(
        &mut self,
        sample: Complex32,
        bfo_hz: f32,
        sample_rate: f32,
        sideband: CwSideband,
    ) -> f32 {
        let mixed = if sideband.mix_up() {
            self.bfo.mix_up(sample, bfo_hz, sample_rate)
        } else {
            self.bfo.mix_down(sample, bfo_hz, sample_rate)
        };
        mixed.re
    }

    pub fn process(
        &mut self,
        sample: Complex32,
        bfo_hz: f32,
        sample_rate: f32,
        sideband: CwSideband,
        mode: CwDetectorMode,
        wpm: f32,
        matched_dit_samples: Option<usize>,
    ) -> f32 {
        self.sync_windows(sample_rate, mode, wpm, matched_dit_samples);
        let integrated = self.push_coherent(sample);
        let env = self.matched_envelope(sample);
        let raw = self.product_demod(integrated, bfo_hz, sample_rate, sideband);
        if mode == CwDetectorMode::MatchedDit {
            raw * (env / (env + 0.02)).min(1.0)
        } else {
            raw
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    #[test]
    fn product_mode_emits_bfo_tone() {
        let rate = 12_000.0;
        let bfo = 650.0;
        let mut det = CwDetector::new();
        let mut peak = 0.0f32;
        for n in 0..rate as usize {
            let t = n as f32 / rate;
            let phase = TAU * bfo * t;
            let sample = Complex32::new(phase.cos(), phase.sin());
            let out = det.process(
                sample,
                bfo,
                rate,
                CwSideband::Lower,
                CwDetectorMode::Product,
                20.0,
                None,
            );
            peak = peak.max(out.abs());
        }
        assert!(peak > 0.5, "peak={peak}");
    }

    #[test]
    fn coherent_mode_produces_audio() {
        let rate = 12_000.0;
        let bfo = 650.0;
        let mut det = CwDetector::new();
        let mut peak = 0.0f32;
        for _ in 0..rate as usize * 2 {
            // After channel filter the keyed carrier sits near DC, not at BFO.
            let sample = Complex32::new(0.15, 0.0);
            let out = det.process(
                sample,
                bfo,
                rate,
                CwSideband::Lower,
                CwDetectorMode::Coherent,
                20.0,
                None,
            );
            peak = peak.max(out.abs());
        }
        assert!(peak > 0.02);
    }
}
