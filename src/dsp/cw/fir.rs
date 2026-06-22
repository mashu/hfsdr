//! Linear-phase windowed-sinc FIR filters for CW channel shaping.
//!
//! Linear phase (symmetric taps) preserves the keying edges so fast CW does not
//! smear or ring. The window choice trades skirt steepness against ringing:
//! Gaussian has essentially no overshoot, Kaiser is tunable, raised-cosine
//! (Hann/Blackman) gives steeper skirts — all far cleaner than elliptic IIR.

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
    /// Kaiser: adjustable β — flat passband vs steep skirts.
    Kaiser,
}

/// Parameters for [`design_lowpass_with`].
#[derive(Clone, Copy, Debug)]
pub struct LowpassDesign {
    pub window: WindowKind,
    /// Kaiser β (3 = wide, 12 = steep). Ignored for other windows.
    pub kaiser_beta: f32,
    /// Convolve with a short inverse-sinc EQ to lift upstream boxcar/CIC droop.
    pub passband_flatten: bool,
}

impl Default for LowpassDesign {
    fn default() -> Self {
        Self {
            window: WindowKind::Gaussian,
            kaiser_beta: 6.0,
            passband_flatten: false,
        }
    }
}

/// Preallocated FIR with a circular delay line — allocation-free after construction.
#[derive(Clone, Debug)]
pub struct FirFilter {
    taps: Vec<f32>,
    delay_i: Vec<f32>,
    delay_q: Vec<f32>,
    pos: usize,
}

/// Maximum filter group delay — longer delays smear CW keying edges.
const MAX_GROUP_DELAY_MS: f32 = 12.0;

/// Tap count for a channel filter (matches [`design_lowpass_with`]).
pub fn plan_num_taps(sample_rate: f32, bandwidth_hz: f32) -> usize {
    let cutoff = (bandwidth_hz * 0.5).max(10.0);
    let mut num_taps = ((sample_rate / cutoff) * 4.0).round() as usize;
    let max_taps_delay =
        ((sample_rate * MAX_GROUP_DELAY_MS / 1000.0) * 2.0).round() as usize | 1;
    num_taps = num_taps.min(max_taps_delay).clamp(31, 2047);
    if num_taps.is_multiple_of(2) {
        num_taps += 1;
    }
    num_taps
}

/// Linear-phase group delay of the channel FIR (~half the tap count).
pub fn channel_group_delay_ms(sample_rate: f32, bandwidth_hz: f32) -> f32 {
    if sample_rate <= 0.0 {
        return 0.0;
    }
    let n = plan_num_taps(sample_rate, bandwidth_hz) as f32;
    (n - 1.0) * 0.5 / sample_rate * 1000.0
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

/// Design a symmetric windowed-sinc lowpass (default options).
pub fn design_lowpass(sample_rate: f32, bandwidth_hz: f32, window: WindowKind) -> FirFilter {
    design_lowpass_with(
        sample_rate,
        bandwidth_hz,
        LowpassDesign {
            window,
            ..LowpassDesign::default()
        },
    )
}

/// Design a symmetric windowed-sinc lowpass for a CW channel of `bandwidth_hz`.
pub fn design_lowpass_with(
    sample_rate: f32,
    bandwidth_hz: f32,
    design: LowpassDesign,
) -> FirFilter {
    let cutoff = (bandwidth_hz * 0.5).max(10.0);
    let num_taps = plan_num_taps(sample_rate, bandwidth_hz);
    let m = (num_taps - 1) as f32;
    let fc = cutoff / sample_rate;
    let beta = design.kaiser_beta.clamp(2.0, 14.0);

    let mut taps = Vec::with_capacity(num_taps);
    let mut sum = 0.0f32;
    for k in 0..num_taps {
        let x = k as f32 - m / 2.0;
        let sinc = if x.abs() < 1e-6 {
            2.0 * fc
        } else {
            (2.0 * PI * fc * x).sin() / (PI * x)
        };
        let tap = sinc * window_value(design.window, k, num_taps, beta);
        taps.push(tap);
        sum += tap;
    }
    if sum.abs() > f32::EPSILON {
        for tap in &mut taps {
            *tap /= sum;
        }
    }

    if design.passband_flatten {
        let comp = design_droop_compensator();
        taps = convolve_centered(&taps, &comp);
    }

    FirFilter::new(taps)
}

/// Gaussian-shaped CW channel filter (default — cleanest tone).
pub fn design_gaussian_lowpass(sample_rate: f32, bandwidth_hz: f32) -> FirFilter {
    design_lowpass(sample_rate, bandwidth_hz, WindowKind::Gaussian)
}

/// Short inverse-sinc compensator for upstream boxcar/CIC passband droop.
/// Based on the liquid-dsp approach: passband gain ∝ 1/sinc(N·f), N ≈ 7.
fn design_droop_compensator() -> Vec<f32> {
    const N: f32 = 7.0;
    const TAPS: usize = 15;
    let m = (TAPS - 1) as f32;
    let pass_edge = 0.85 / N;

    let mut taps = vec![0.0f32; TAPS];
    for i in 0..TAPS {
        let mut acc = 0.0f32;
        for j in 0..TAPS {
            let f = j as f32 / TAPS as f32 * 0.5;
            let gain = if f <= pass_edge {
                let v = normalized_sinc(N * f);
                (1.0 / v).clamp(1.0, 3.5)
            } else {
                0.0
            };
            let phase = 2.0 * PI * (i as f32) * (j as f32 - m / 2.0) / TAPS as f32;
            acc += gain * phase.cos();
        }
        let hann = 0.5 - 0.5 * (2.0 * PI * i as f32 / m).cos();
        taps[i] = acc / TAPS as f32 * hann;
    }
    let sum: f32 = taps.iter().sum();
    if sum.abs() > f32::EPSILON {
        for t in &mut taps {
            *t /= sum;
        }
    }
    taps
}

fn convolve_centered(main: &[f32], eq: &[f32]) -> Vec<f32> {
    let out_len = main.len();
    let full_len = main.len() + eq.len() - 1;
    let mut full = vec![0.0f32; full_len];
    for (i, &a) in main.iter().enumerate() {
        for (j, &b) in eq.iter().enumerate() {
            full[i + j] += a * b;
        }
    }
    let start = (full_len - out_len) / 2;
    let mut out = full[start..start + out_len].to_vec();
    let sum: f32 = out.iter().sum();
    if sum.abs() > f32::EPSILON {
        for t in &mut out {
            *t /= sum;
        }
    }
    out
}

fn normalized_sinc(x: f32) -> f32 {
    if x.abs() < 1e-6 {
        1.0
    } else {
        (PI * x).sin() / (PI * x)
    }
}

fn window_value(window: WindowKind, k: usize, num_taps: usize, kaiser_beta: f32) -> f32 {
    let m = (num_taps - 1) as f32;
    let kf = k as f32;
    match window {
        WindowKind::Gaussian => {
            let sigma = (m / 2.0) / 3.0;
            let x = kf - m / 2.0;
            (-0.5 * (x / sigma).powi(2)).exp()
        }
        WindowKind::RaisedCosine => 0.5 - 0.5 * (2.0 * PI * kf / m).cos(),
        WindowKind::Blackman => {
            0.42 - 0.5 * (2.0 * PI * kf / m).cos() + 0.08 * (4.0 * PI * kf / m).cos()
        }
        WindowKind::Kaiser => kaiser_window(kf, m, kaiser_beta),
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
        if term < 1e-8 {
            break;
        }
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    fn stopband_db(design: LowpassDesign, bw: f32, probe_hz: f32) -> f32 {
        let rate = 12_000.0;
        let mut pass = design_lowpass_with(rate, bw, design);
        let mut stop = design_lowpass_with(rate, bw, design);
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
        let db = stopband_db(LowpassDesign::default(), 200.0, 600.0);
        assert!(db < -40.0, "stopband only {db} dB");
    }

    #[test]
    fn kaiser_high_beta_rejects_adjacent() {
        let db = stopband_db(
            LowpassDesign {
                window: WindowKind::Kaiser,
                kaiser_beta: 10.0,
                passband_flatten: false,
            },
            200.0,
            600.0,
        );
        assert!(db < -45.0, "Kaiser β=10 stopband only {db} dB");
    }

    #[test]
    fn flatten_preserves_dc() {
        let rate = 12_000.0;
        let mut fir = design_lowpass_with(
            rate,
            200.0,
            LowpassDesign {
                passband_flatten: true,
                ..LowpassDesign::default()
            },
        );
        let mut peak = 0.0f32;
        for _ in 0..rate as usize {
            peak = peak.max(fir.process_complex(Complex32 { re: 1.0, im: 0.0 }).re.abs());
        }
        assert!(peak > 0.85, "flatten DC gain too low: {peak}");
    }

    #[test]
    fn ultra_narrow_caps_group_delay() {
        let ms = channel_group_delay_ms(12_000.0, 50.0);
        assert!(ms <= MAX_GROUP_DELAY_MS + 1.0, "delay {ms} ms smears keying");
    }

    #[test]
    fn blackman_narrow_rejects_close_adjacent() {
        let db = stopband_db(
            LowpassDesign {
                window: WindowKind::Blackman,
                ..Default::default()
            },
            100.0,
            250.0,
        );
        assert!(db < -35.0, "250 Hz tone with 100 Hz BW: {db} dB");
    }
}
