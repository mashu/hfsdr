//! Linear-phase windowed-sinc FIR filters for CW channel shaping.
//!
//! Linear phase (symmetric taps) preserves the keying edges so fast CW does not
//! smear or ring. The window choice trades skirt steepness against ringing:
//! Gaussian has essentially no overshoot, raised-cosine (Hann/Blackman) gives
//! slightly steeper skirts with negligible ring — both far cleaner than elliptic
//! IIR designs.

use std::f32::consts::PI;

use crate::source::Complex32;

/// Window applied to the ideal sinc — the "shape" of the CW filter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowKind {
    /// Gaussian taper: no ringing, gentle skirts. Best tone purity.
    Gaussian,
    /// Raised-cosine (Hann): the accurate/selective compromise.
    RaisedCosine,
    /// Blackman: steepest clean skirts, slightly wider transition.
    Blackman,
}

/// Preallocated FIR with a circular delay line — allocation-free after construction.
#[derive(Clone, Debug)]
pub struct FirFilter {
    taps: Vec<f32>,
    delay_i: Vec<f32>,
    delay_q: Vec<f32>,
    pos: usize,
}

impl FirFilter {
    pub fn new(taps: Vec<f32>) -> Self {
        let len = taps.len().max(1);
        Self {
            taps,
            delay_i: vec![0.0; len],
            delay_q: vec![0.0; len],
            pos: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.taps.len()
    }

    pub fn is_empty(&self) -> bool {
        self.taps.is_empty()
    }

    pub fn reset_state(&mut self) {
        self.delay_i.fill(0.0);
        self.delay_q.fill(0.0);
        self.pos = 0;
    }

    pub fn process_complex(&mut self, sample: Complex32) -> Complex32 {
        let n = self.taps.len();
        if n == 0 {
            return sample;
        }
        self.delay_i[self.pos] = sample.re;
        self.delay_q[self.pos] = sample.im;

        let mut acc_i = 0.0f32;
        let mut acc_q = 0.0f32;
        let mut idx = self.pos;
        for &tap in &self.taps {
            acc_i += self.delay_i[idx] * tap;
            acc_q += self.delay_q[idx] * tap;
            idx = if idx == 0 { n - 1 } else { idx - 1 };
        }

        self.pos = if self.pos + 1 == n { 0 } else { self.pos + 1 };
        Complex32 {
            re: acc_i,
            im: acc_q,
        }
    }
}

/// Design a symmetric windowed-sinc lowpass for a CW channel of `bandwidth_hz`.
///
/// `bandwidth_hz` is the full passband width, so the lowpass cutoff is half that
/// (the wanted signal sits at DC after the NCO mixes it down). Taps are causal
/// order: `taps[0]` weights the newest sample.
pub fn design_lowpass(sample_rate: f32, bandwidth_hz: f32, window: WindowKind) -> FirFilter {
    let cutoff = (bandwidth_hz * 0.5).max(10.0);
    let mut num_taps = ((sample_rate / cutoff) * 4.0).round() as usize;
    num_taps = num_taps.clamp(31, 2047);
    if num_taps.is_multiple_of(2) {
        num_taps += 1;
    }

    let m = (num_taps - 1) as f32;
    let fc = cutoff / sample_rate;

    let mut taps = Vec::with_capacity(num_taps);
    let mut sum = 0.0f32;
    for k in 0..num_taps {
        let x = k as f32 - m / 2.0;
        let sinc = if x.abs() < 1e-6 {
            2.0 * fc
        } else {
            (2.0 * PI * fc * x).sin() / (PI * x)
        };
        let tap = sinc * window_value(window, k, num_taps);
        taps.push(tap);
        sum += tap;
    }
    if sum.abs() > f32::EPSILON {
        for tap in &mut taps {
            *tap /= sum;
        }
    }

    FirFilter::new(taps)
}

/// Gaussian-shaped CW channel filter (default — cleanest tone).
pub fn design_gaussian_lowpass(sample_rate: f32, bandwidth_hz: f32) -> FirFilter {
    design_lowpass(sample_rate, bandwidth_hz, WindowKind::Gaussian)
}

fn window_value(window: WindowKind, k: usize, num_taps: usize) -> f32 {
    let m = (num_taps - 1) as f32;
    let kf = k as f32;
    match window {
        WindowKind::Gaussian => {
            // sigma chosen for ~>40 dB stopband while staying ring-free.
            let sigma = (m / 2.0) / 3.0;
            let x = kf - m / 2.0;
            (-0.5 * (x / sigma).powi(2)).exp()
        }
        WindowKind::RaisedCosine => 0.5 - 0.5 * (2.0 * PI * kf / m).cos(),
        WindowKind::Blackman => {
            0.42 - 0.5 * (2.0 * PI * kf / m).cos() + 0.08 * (4.0 * PI * kf / m).cos()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    fn stopband_db(window: WindowKind, bw: f32, probe_hz: f32) -> f32 {
        let rate = 12_000.0;
        let mut pass = design_lowpass(rate, bw, window);
        let mut stop = design_lowpass(rate, bw, window);
        let warmup = pass.len() * 2;
        let mut pass_pow = 0.0f32;
        let mut stop_pow = 0.0f32;
        let mut count = 0usize;
        for n in 0..rate as usize * 2 {
            let t = n as f32 / rate;
            let p = pass.process_complex(Complex32 { re: 1.0, im: 0.0 });
            let tone = Complex32 {
                re: (TAU * probe_hz * t).cos(),
                im: (TAU * probe_hz * t).sin(),
            };
            let s = stop.process_complex(tone);
            if n >= warmup {
                pass_pow += p.norm().powi(2);
                stop_pow += s.norm().powi(2);
                count += 1;
            }
        }
        let pass_rms = (pass_pow / count as f32).sqrt();
        let stop_rms = (stop_pow / count as f32).sqrt();
        20.0 * (stop_rms / pass_rms.max(1e-9)).log10()
    }

    #[test]
    fn gaussian_passes_dc() {
        let rate = 12_000.0;
        let mut fir = design_gaussian_lowpass(rate, 200.0);
        let mut peak = 0.0f32;
        for _ in 0..rate as usize {
            peak = peak.max(fir.process_complex(Complex32 { re: 1.0, im: 0.0 }).re.abs());
        }
        assert!(peak > 0.9, "DC gain too low: {peak}");
    }

    #[test]
    fn gaussian_strong_stopband() {
        // 200 Hz channel: a 600 Hz interferer must be deep in the stopband.
        let db = stopband_db(WindowKind::Gaussian, 200.0, 600.0);
        assert!(db < -40.0, "stopband only {db} dB");
    }

    #[test]
    fn blackman_narrow_rejects_close_adjacent() {
        let db = stopband_db(WindowKind::Blackman, 100.0, 250.0);
        assert!(db < -35.0, "250 Hz tone with 100 Hz BW: {db} dB");
    }
}
