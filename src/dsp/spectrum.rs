//! Windowed, overlapping complex FFT that turns baseband IQ into fftshifted
//! **amplitude** spectrum rows (dBFS), not power spectral density.
//!
//! Normalisation is coherent: `|X| / sum(window)`. A full-scale complex tone
//! reads 0 dBFS at any FFT size, which is what carrier levels and the S-meter
//! depend on. The trade-off is that broadband noise, being spread across bins,
//! falls 3 dB per FFT-size doubling — see `noise_floor_falls_3db_per_doubling`.
//! Skimmer thresholds are SNR-relative (peak vs. floor) and so are unaffected.

use crate::source::Complex32;

use super::fft_plan::plan_forward;
use super::fft_window::{build_fft_window, DEFAULT_FFT_WINDOW, FftWindowKind};
use super::cw::DEFAULT_KAISER_BETA;
use rustfft::Fft;
use std::sync::Arc;

/// Frames of backlog retained when a caller's row cap stops emission mid-batch.
/// Bounds how far the analyzer can lag live IQ before it starts dropping.
const MAX_PENDING_FRAMES: usize = 8;

/// Fixed-capacity ring for sliding-window FFT input. Avoids O(n) `Vec::drain`
/// on every hop when sample rates are high.
///
/// Capacity is one FFT frame plus [`MAX_PENDING_FRAMES`] hops of backlog, so a
/// row-capped call can retain its unemitted frames for the next call. Input
/// beyond that bound overwrites the oldest sample and is counted in `dropped`.
struct SampleRing {
    data: Vec<Complex32>,
    head: usize,
    count: usize,
    capacity: usize,
    dropped: u64,
}

impl SampleRing {
    fn new(capacity: usize) -> Self {
        Self {
            data: vec![Complex32::new(0.0, 0.0); capacity],
            head: 0,
            count: 0,
            capacity,
            dropped: 0,
        }
    }

    fn push(&mut self, sample: Complex32) {
        if self.count < self.capacity {
            let tail = (self.head + self.count) % self.capacity;
            self.data[tail] = sample;
            self.count += 1;
        } else {
            // Backlog bound reached: overwrite the oldest sample and count it.
            self.data[self.head] = sample;
            self.head = (self.head + 1) % self.capacity;
            self.dropped += 1;
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

    fn clear(&mut self) {
        self.head = 0;
        self.count = 0;
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
        Self::with_window(n, hop, DEFAULT_FFT_WINDOW, DEFAULT_KAISER_BETA)
    }

    pub fn with_window(n: usize, hop: usize, window_kind: FftWindowKind, kaiser_beta: f32) -> Self {
        assert!(n.is_power_of_two(), "FFT size should be a power of two");
        let hop = hop.clamp(1, n);
        let fft = plan_forward(n);
        let scratch = vec![Complex32::new(0.0, 0.0); fft.get_inplace_scratch_len()];

        let window = build_fft_window(n, window_kind, kaiser_beta);
        let coherent_gain = window.iter().sum::<f32>() / n as f32;

        Self {
            fft,
            n,
            hop,
            window,
            coherent_gain,
            acc: SampleRing::new(n + hop * MAX_PENDING_FRAMES),
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
    ///
    /// Frames left unemitted by the cap are retained and produced on later calls,
    /// including calls with empty `input` — pass `&[]` to drain the backlog. The
    /// backlog is bounded at [`MAX_PENDING_FRAMES`] frames; a caller that stays
    /// below the incoming row rate for longer than that loses the oldest samples,
    /// counted by [`Self::dropped_samples`].
    pub fn process_limited<F: FnMut(&[f32])>(
        &mut self,
        input: &[Complex32],
        max_rows: usize,
        mut emit: F,
    ) -> usize {
        let mut emitted = 0usize;
        for &sample in input {
            self.acc.push(sample);
            while emitted < max_rows && self.acc.len() >= self.n {
                self.emit_row(&mut emit);
                emitted += 1;
            }
        }
        // Drain frames retained from earlier row-capped calls. This is the only
        // path taken when `input` is empty, so it must live outside the loop above.
        while emitted < max_rows && self.acc.len() >= self.n {
            self.emit_row(&mut emit);
            emitted += 1;
        }
        emitted
    }

    /// Discard buffered samples because the incoming stream jumped.
    ///
    /// Call this whenever the caller skipped input rather than feeding it —
    /// catching up by dropping old IQ, retuning, reconnecting. The analyzer
    /// accumulates across calls and its windows overlap, so without this the
    /// samples either side of the gap end up inside one FFT window. That window
    /// transforms a step discontinuity, not a signal: its noise floor rises by
    /// tens of dB and the row paints as a bright band across the waterfall.
    ///
    /// The cost is one dropped row's worth of latency at the jump, which is the
    /// correct trade — the alternative is a row of pure artifact.
    pub fn reset(&mut self) {
        self.acc.clear();
    }

    /// Total IQ samples lost to the backlog bound since construction.
    pub fn dropped_samples(&self) -> u64 {
        self.acc.dropped
    }

    /// Frames currently buffered and ready to emit without further input.
    pub fn pending_rows(&self) -> usize {
        if self.acc.len() < self.n {
            return 0;
        }
        (self.acc.len() - self.n) / self.hop + 1
    }

    /// Window, transform and fftshift one frame, then advance by one hop.
    fn emit_row<F: FnMut(&[f32])>(&mut self, emit: &mut F) {
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
        // Coherent normalisation: a full-scale tone reads 0 dBFS at any FFT size.
        let norm = self.n as f32 * self.coherent_gain;
        for i in 0..self.n {
            let src = (i + half) % self.n;
            let re = self.buf[src].re;
            let im = self.buf[src].im;
            let mag = (re * re + im * im).sqrt() / norm;
            self.row[i] = 20.0 * (mag + 1e-12).log10();
        }
        emit(&self.row);
        self.acc.advance(self.hop);
    }
}

/// Skipping input must not corrupt the rows either side of the skip.
///
/// The engine deliberately drops old IQ from the spectrum path to keep the
/// waterfall showing what is being heard. That is fine; splicing across the
/// gap is not. The analyzer's windows overlap, so a window spanning the seam
/// transforms a step rather than the signal, and paints as a bright band.
#[cfg(test)]
mod discontinuity_tests {
    use super::*;
    use std::f32::consts::TAU;

    fn tone(n: usize, rate: f32, hz: f32, phase0: usize) -> Vec<Complex32> {
        (0..n)
            .map(|i| {
                let ph = TAU * hz * (i + phase0) as f32 / rate;
                Complex32::new(ph.cos() * 0.5, ph.sin() * 0.5)
            })
            .collect()
    }

    /// Median bin of every row, which is the noise floor a splice inflates.
    fn row_floors(reset_at_gap: bool) -> Vec<f32> {
        let mut analyzer = SpectrumAnalyzer::new(2048, 1024);
        let mut floors = Vec::new();
        let mut emit = |row: &[f32], floors: &mut Vec<f32>| {
            let mut v: Vec<f32> = row.to_vec();
            v.sort_by(|a, b| a.partial_cmp(b).unwrap());
            floors.push(v[v.len() / 2]);
        };

        let mut phase = 0usize;
        for block in 0..12 {
            // Every third block, skip 3000 samples of input — the engine
            // discarding backlog to stay live.
            if block % 3 == 2 {
                // Not a whole number of tone periods: a skip that is one would
                // be phase-continuous and leave nothing to detect.
                phase += 3001;
                if reset_at_gap {
                    analyzer.reset();
                }
            }
            let chunk = tone(2048, 12_000.0, 1533.0, phase);
            phase += 2048;
            analyzer.process(&chunk, |row| emit(row, &mut floors));
        }
        floors
    }

    /// Largest jump between consecutive rows — banding is the jump, and the
    /// floor drifts slowly on its own as f32 phase precision accumulates.
    fn worst_jump(floors: &[f32]) -> f32 {
        floors
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max)
    }

    #[test]
    fn splicing_across_a_skip_wrecks_the_noise_floor() {
        let spliced = row_floors(false);
        assert!(
            worst_jump(&spliced) > 40.0,
            "expected splice damage to be visible (worst jump {:.1} dB) — if this no \
             longer holds, the premise behind `reset` has changed",
            worst_jump(&spliced)
        );
    }

    #[test]
    fn resetting_at_the_skip_keeps_every_row_clean() {
        let clean = row_floors(true);
        assert!(clean.len() > 6, "only {} rows", clean.len());
        let jump = worst_jump(&clean);
        assert!(
            jump < 15.0,
            "rows still jump {jump:.1} dB after resetting at the gap — the waterfall \
             would band"
        );
    }
}

/// A stationary input must produce stationary rows.
///
/// The browser waterfall showed heavy horizontal banding on a constant
/// multi-tone signal — consecutive rows alternating between saturated and
/// near-black. Banding is either the analyzer producing rows that really do
/// swing like that, or something further down inventing it, and the row data
/// is where that gets decided.
#[cfg(test)]
mod stationarity_tests {
    use super::*;
    use std::f32::consts::TAU;

    /// Several strong carriers summed, near full scale.
    fn multitone(n: usize, rate: f32, phase0: usize) -> Vec<Complex32> {
        (0..n)
            .map(|i| {
                let t = (i + phase0) as f32 / rate;
                let mut re = 0.0;
                let mut im = 0.0;
                for (hz, amp) in [(-2200.0f32, 0.25f32), (-800.0, 0.35), (-50.0, 0.30)] {
                    let ph = TAU * hz * t;
                    re += ph.cos() * amp;
                    im += ph.sin() * amp;
                }
                Complex32::new(re, im)
            })
            .collect()
    }

    /// Peak dB of every row produced, feeding the analyzer in realistic chunks.
    fn row_peaks(chunk: usize, chunks: usize) -> Vec<f32> {
        let mut analyzer = SpectrumAnalyzer::new(2048, 1024);
        let mut peaks = Vec::new();
        for c in 0..chunks {
            let block = multitone(chunk, 12_000.0, c * chunk);
            analyzer.process(&block, |row| {
                peaks.push(row.iter().copied().fold(f32::NEG_INFINITY, f32::max));
            });
        }
        peaks
    }

    #[test]
    fn constant_signal_gives_rows_of_constant_level() {
        // 512 samples is roughly one KiwiSDR SND frame.
        let peaks = row_peaks(512, 200);
        assert!(peaks.len() > 40, "only {} rows", peaks.len());

        let settled = &peaks[4..];
        let max = settled.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let min = settled.iter().copied().fold(f32::INFINITY, f32::min);
        assert!(
            max - min < 1.0,
            "row peaks span {:.2} dB on a constant signal (min {min:.2}, max {max:.2})",
            max - min
        );

        // Banding is the row-to-row alternation, not the overall spread.
        let worst = settled
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max);
        assert!(worst < 0.5, "consecutive rows differ by {worst:.2} dB");
    }

    /// Chunk size must not change the result: the same signal delivered in
    /// different-sized blocks has to produce the same rows, or the display
    /// depends on network packet boundaries rather than on the signal.
    #[test]
    fn row_levels_do_not_depend_on_chunk_size() {
        let level = |chunk: usize| {
            let peaks = row_peaks(chunk, 200 * 512 / chunk);
            let settled = &peaks[4..];
            settled.iter().sum::<f32>() / settled.len() as f32
        };
        let small = level(256);
        let large = level(2048);
        assert!(
            (small - large).abs() < 0.5,
            "mean row level depends on chunk size: {small:.2} dB at 256, {large:.2} at 2048"
        );
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

    /// The row cap must defer frames, not discard them. Draining with EMPTY input
    /// is the only form of this test that can fail if samples are dropped — feeding
    /// fresh input on the second call would pass either way.
    #[test]
    fn process_limited_defers_frames_instead_of_dropping_them() {
        let n = 64;
        let hop = n;
        let mut sa = SpectrumAnalyzer::new(n, hop);
        let frames = 6;
        let input = vec![Complex32::new(0.5, 0.0); n * frames];

        let mut first = 0usize;
        sa.process_limited(&input, 2, |_| first += 1);
        assert_eq!(first, 2, "cap honoured on the first call");
        assert_eq!(sa.pending_rows(), frames - 2, "remainder retained");

        let mut drained = 0usize;
        for _ in 0..frames {
            sa.process_limited(&[], 2, |_| drained += 1);
        }
        assert_eq!(drained, frames - 2, "every deferred frame emitted on empty input");
        assert_eq!(sa.dropped_samples(), 0, "nothing dropped within the backlog bound");
    }

    /// Past the backlog bound, dropping is explicit and counted rather than silent.
    #[test]
    fn backlog_bound_drops_are_counted() {
        let n = 64;
        let hop = n;
        let mut sa = SpectrumAnalyzer::new(n, hop);
        let input = vec![Complex32::new(0.5, 0.0); n * (MAX_PENDING_FRAMES + 6)];
        sa.process_limited(&input, 1, |_| {});
        assert!(sa.dropped_samples() > 0, "overflow past the bound is reported");
        assert!(
            sa.pending_rows() <= MAX_PENDING_FRAMES + 1,
            "backlog stays bounded, so latency cannot grow without limit"
        );
    }

    /// Phase-3 invariant A: coherent normalisation keeps tone level FFT-size independent.
    #[test]
    fn tone_level_is_independent_of_fft_size() {
        use std::f32::consts::TAU;
        let sr = 48_000.0;
        let freq = 3_000.0;
        let mut levels = Vec::new();
        for &n in &[1024usize, 2048, 4096] {
            let mut sa = SpectrumAnalyzer::new(n, n);
            let samples: Vec<Complex32> = (0..n * 4)
                .map(|t| {
                    let p = TAU * freq * t as f32 / sr;
                    Complex32::new(p.cos(), p.sin())
                })
                .collect();
            let mut peak = -200.0f32;
            sa.process(&samples, |row| {
                peak = peak.max(row.iter().copied().fold(-200.0f32, f32::max));
            });
            levels.push(peak);
        }
        for w in levels.windows(2) {
            assert!(
                (w[0] - w[1]).abs() < 0.5,
                "full-scale tone must read ~0 dBFS at every FFT size, got {levels:?}"
            );
        }
        assert!((levels[0]).abs() < 0.5, "full-scale tone reads ~0 dBFS");
    }

    /// Phase-3 invariant B: the documented consequence — the noise floor is NOT
    /// FFT-size independent. Asserted so the trade-off stays deliberate.
    #[test]
    fn noise_floor_falls_3db_per_doubling() {
        let mut state = 0x12345678u32;
        let mut rng = move || {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            (state >> 8) as f32 / 8388608.0 - 1.0
        };
        let samples: Vec<Complex32> = (0..1 << 16)
            .map(|_| Complex32::new(rng() * 0.1, rng() * 0.1))
            .collect();

        let mut floors = Vec::new();
        for &n in &[2048usize, 4096] {
            let mut sa = SpectrumAnalyzer::new(n, n);
            let (mut sum, mut cnt) = (0.0f64, 0usize);
            sa.process(&samples, |row| {
                sum += row.iter().map(|&v| v as f64).sum::<f64>() / row.len() as f64;
                cnt += 1;
            });
            floors.push(sum / cnt.max(1) as f64);
        }
        let delta = floors[0] - floors[1];
        assert!(
            (delta - 3.01).abs() < 0.4,
            "expected ~3.01 dB drop per doubling (amplitude spectrum), got {delta:.2}"
        );
    }
}
