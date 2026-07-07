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
    dit_power_buf: Vec<f32>,
    dit_power_sum: f32,
    dit_pos: usize,
    dit_len: usize,
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
            dit_power_buf: Vec::new(),
            dit_power_sum: 0.0,
            dit_pos: 0,
            dit_len: 1,
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
        let coherent_len = coherent_len.max(1);
        if coherent_len != self.coherent_len {
            // Preserve ring contents: the WPM estimate refines continuously, so
            // the window resizes by a sample or two — zeroing the state here
            // collapses the integrator and dents the audio on every change.
            resize_ring_oldest_first(
                &mut self.coherent_buf,
                &mut self.coherent_pos,
                coherent_len,
                Complex32::new(0.0, 0.0),
            );
            self.coherent_sum = self
                .coherent_buf
                .iter()
                .fold(Complex32::new(0.0, 0.0), |acc, z| {
                    Complex32::new(acc.re + z.re, acc.im + z.im)
                });
            self.coherent_len = coherent_len;
        }
        let dit_len = if mode == CwDetectorMode::MatchedDit {
            matched_dit_samples
                .unwrap_or_else(|| dit_samples(wpm, sample_rate))
                .max(1)
        } else {
            1
        };
        if dit_len != self.dit_len {
            resize_ring_oldest_first(&mut self.dit_power_buf, &mut self.dit_pos, dit_len, 0.0);
            self.dit_power_sum = self.dit_power_buf.iter().sum();
            self.dit_len = dit_len;
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

/// Resize a ring buffer, keeping the newest samples and leaving `pos` pointing
/// at the oldest slot (the next write target). Grows pad with `fill` as the
/// oldest entries; shrinks drop the oldest entries.
fn resize_ring_oldest_first<T: Copy>(buf: &mut Vec<T>, pos: &mut usize, new_len: usize, fill: T) {
    let old_len = buf.len();
    if old_len == new_len {
        return;
    }
    if old_len == 0 {
        buf.resize(new_len, fill);
        *pos = 0;
        return;
    }
    // Reorder in place to oldest..newest, then trim/pad at the front.
    buf.rotate_left(*pos % old_len);
    if new_len < old_len {
        buf.drain(..old_len - new_len);
    } else {
        buf.splice(0..0, std::iter::repeat_n(fill, new_len - old_len));
    }
    *pos = 0;
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
    fn matched_dit_window_resize_preserves_envelope() {
        // The WPM estimate jitters the dit window by a sample or two; the
        // matched-envelope integrator must carry its state across resizes
        // instead of collapsing to zero (which dents the audio).
        let rate = 12_000.0;
        let mut det = CwDetector::new();
        let carrier = Complex32::new(0.2, 0.0);
        for _ in 0..600 {
            let _ = det.process(
                carrier,
                650.0,
                rate,
                CwSideband::Lower,
                CwDetectorMode::MatchedDit,
                20.0,
                Some(90),
            );
        }
        // One sample after the window grows, the envelope must still be hot.
        let _ = det.process(
            carrier,
            650.0,
            rate,
            CwSideband::Lower,
            CwDetectorMode::MatchedDit,
            20.0,
            Some(92),
        );
        let env = (det.dit_power_sum / det.dit_len as f32).sqrt();
        assert!(
            env > 0.15,
            "matched envelope collapsed on window resize: {env}"
        );
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
