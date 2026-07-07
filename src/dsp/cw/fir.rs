//! Linear-phase windowed-sinc FIR filters for CW channel shaping.
//!
//! Linear phase (symmetric taps) preserves the keying edges so fast CW does not
//! smear or ring. The window choice trades skirt steepness against ringing:
//! Gaussian has essentially no overshoot, Kaiser is tunable, raised-cosine
//! (Hann/Blackman) gives steeper skirts — all far cleaner than elliptic IIR.

use std::f32::consts::PI;

use num_complex::Complex;
use crate::source::Complex32;

use super::super::fft_plan::{plan_forward, plan_inverse};
use super::super::simd::dot_f32;
use super::filter_plan::{
    self, plan_num_taps, passband_cutoff_hz, DEFAULT_PASSBAND_CUTOFF_FRAC, MAX_DOLPH_SIDELOBE_DB,
    MAX_KAISER_BETA, MIN_DOLPH_SIDELOBE_DB, MIN_KAISER_BETA,
};

/// Use FFT overlap convolution for blocks at least this long (with long enough taps).
const FFT_BLOCK_MIN_INPUT: usize = 128;
const FFT_BLOCK_MIN_TAPS: usize = 64;

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
    /// Dolph–Chebyshev: equiripple stopband — steepest skirts for a given tap count.
    DolphChebyshev,
}

/// Parameters for [`design_lowpass_with`].
#[derive(Clone, Copy, Debug)]
pub struct LowpassDesign {
    pub window: WindowKind,
    /// Kaiser β (3 = wide, 12 = steep). Ignored for other windows.
    pub kaiser_beta: f32,
    /// Convolve with a short inverse-sinc EQ to lift upstream boxcar/CIC droop.
    pub passband_flatten: bool,
    /// Sinc cutoff as a fraction of GUI passband width.
    pub cutoff_frac: f32,
    /// Allow maximum group-delay budget for sharper skirts.
    pub deep_selectivity: bool,
    /// Target sidelobe attenuation (dB) for [`WindowKind::DolphChebyshev`].
    pub dolph_sidelobe_db: f32,
}

impl Default for LowpassDesign {
    fn default() -> Self {
        Self {
            window: WindowKind::Gaussian,
            kaiser_beta: filter_plan::DEFAULT_KAISER_BETA,
            passband_flatten: false,
            cutoff_frac: DEFAULT_PASSBAND_CUTOFF_FRAC,
            deep_selectivity: false,
            dolph_sidelobe_db: filter_plan::DEFAULT_DOLPH_SIDELOBE_DB,
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
    h_fft: Vec<Complex<f32>>,
    fft_n: usize,
    scratch: Vec<Complex<f32>>,
    hist: Vec<Complex<f32>>,
}

impl FirFilter {
    pub fn new(taps: Vec<f32>) -> Self {
        let len = taps.len().max(1);
        Self {
            taps,
            delay_i: vec![0.0; len],
            delay_q: vec![0.0; len],
            pos: 0,
            h_fft: Vec::new(),
            fft_n: 0,
            scratch: Vec::new(),
            hist: Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.taps.len()
    }

    pub fn is_empty(&self) -> bool {
        self.taps.is_empty()
    }

    pub fn taps(&self) -> &[f32] {
        &self.taps
    }

    pub fn reset_state(&mut self) {
        self.delay_i.fill(0.0);
        self.delay_q.fill(0.0);
        self.pos = 0;
    }

    /// Block FIR via overlap-save (FFT when profitable); carries delay state
    /// across calls like [`Self::process_complex`].
    ///
    /// The FFT size depends only on the tap count, so the filter spectrum is
    /// computed once per design, not per input-length change; long inputs are
    /// processed in fixed-size segments. Real taps commute with complex input,
    /// so I and Q ride through a single complex convolution.
    pub fn process_complex_block(
        &mut self,
        input: &[Complex32],
        output: &mut Vec<Complex32>,
    ) {
        output.clear();
        if input.is_empty() {
            return;
        }
        if self.taps.is_empty() {
            output.extend_from_slice(input);
            return;
        }
        if input.len() < FFT_BLOCK_MIN_INPUT || self.taps.len() < FFT_BLOCK_MIN_TAPS {
            output.reserve(input.len());
            for &sample in input {
                output.push(self.process_complex(sample));
            }
            return;
        }

        let l = self.taps.len();
        let fft_n = fft_size_for_taps(l);
        let seg = fft_n - (l - 1);
        self.ensure_h_fft(fft_n);

        // Linearize the delay line into complex history (oldest first).
        self.hist.clear();
        self.hist.resize(l - 1, Complex::new(0.0, 0.0));
        let dlen = self.delay_i.len();
        for k in 0..l - 1 {
            let idx = (self.pos + dlen - 1 - k) % dlen;
            self.hist[l - 2 - k] = Complex::new(self.delay_i[idx], self.delay_q[idx]);
        }

        output.resize(input.len(), Complex32 { re: 0.0, im: 0.0 });
        let mut done = 0usize;
        while done < input.len() {
            let chunk = (input.len() - done).min(seg);
            self.scratch.resize(fft_n, Complex::new(0.0, 0.0));
            self.scratch.fill(Complex::new(0.0, 0.0));
            self.scratch[..l - 1].copy_from_slice(&self.hist);
            self.scratch[l - 1..l - 1 + chunk].copy_from_slice(&input[done..done + chunk]);
            plan_forward(fft_n).process(&mut self.scratch);
            for (s, h) in self.scratch.iter_mut().zip(self.h_fft.iter()) {
                *s *= h;
            }
            plan_inverse(fft_n).process(&mut self.scratch);
            let scale = 1.0 / fft_n as f32;
            // input[done] sits at ext index l-1, so its streaming-equivalent
            // output is the linear convolution at l-1.
            for k in 0..chunk {
                output[done + k] = self.scratch[l - 1 + k] * scale;
            }
            // Advance history: newest l-1 samples ending at input[done+chunk-1].
            if chunk >= l - 1 {
                self.hist
                    .copy_from_slice(&input[done + chunk - (l - 1)..done + chunk]);
            } else {
                self.hist.copy_within(chunk.., 0);
                let keep = l - 1 - chunk;
                self.hist[keep..].copy_from_slice(&input[done..done + chunk]);
            }
            done += chunk;
        }

        // Reseed the shared delay line so scalar processing can continue.
        self.delay_i.fill(0.0);
        self.delay_q.fill(0.0);
        for (k, h) in self.hist.iter().enumerate() {
            self.delay_i[k] = h.re;
            self.delay_q[k] = h.im;
        }
        self.pos = (l - 1) % dlen;
    }

    fn ensure_h_fft(&mut self, fft_n: usize) {
        if self.fft_n == fft_n && self.h_fft.len() == fft_n {
            return;
        }
        self.h_fft.clear();
        self.h_fft.resize(fft_n, Complex::new(0.0, 0.0));
        for (i, &t) in self.taps.iter().enumerate() {
            self.h_fft[i] = Complex::new(t, 0.0);
        }
        plan_forward(fft_n).process(&mut self.h_fft);
        self.fft_n = fft_n;
    }

    pub fn process_complex(&mut self, sample: Complex32) -> Complex32 {
        self.feed_and_maybe_emit(sample, true)
            .expect("process_complex always emits")
    }

    /// Store one sample in the delay line; compute the FIR output only when `emit` is true.
    ///
    /// Used by integer decimators so anti-alias MACs run once per output sample, not per
    /// input sample — mathematically identical to full-rate FIR followed by downsampling.
    pub fn feed_and_maybe_emit(&mut self, sample: Complex32, emit: bool) -> Option<Complex32> {
        let n = self.taps.len();
        if n == 0 {
            return if emit { Some(sample) } else { None };
        }
        self.delay_i[self.pos] = sample.re;
        self.delay_q[self.pos] = sample.im;

        let out = if emit {
            let (acc_i, acc_q) = if n >= 32 {
                (
                    fir_dot_delay(&self.delay_i, &self.taps, self.pos, n),
                    fir_dot_delay(&self.delay_q, &self.taps, self.pos, n),
                )
            } else {
                let mut acc_i = 0.0f32;
                let mut acc_q = 0.0f32;
                let mut idx = self.pos;
                for &tap in &self.taps {
                    acc_i += self.delay_i[idx] * tap;
                    acc_q += self.delay_q[idx] * tap;
                    idx = if idx == 0 { n - 1 } else { idx - 1 };
                }
                (acc_i, acc_q)
            };
            Some(Complex32 {
                re: acc_i,
                im: acc_q,
            })
        } else {
            None
        };

        self.pos = if self.pos + 1 == n { 0 } else { self.pos + 1 };
        out
    }
}

/// Overlap-save FFT size for a tap count — fixed per filter design so the
/// filter spectrum is cached regardless of how block lengths vary.
fn fft_size_for_taps(taps_len: usize) -> usize {
    (4 * taps_len).next_power_of_two().max(256)
}

/// Reverse-order delay line dot product (linear-phase FIR).
#[inline]
fn fir_dot_delay(delay: &[f32], taps: &[f32], pos: usize, n: usize) -> f32 {
    if n >= 32 {
        const CHUNK: usize = 32;
        let mut ti = [0.0f32; CHUNK];
        let mut di = [0.0f32; CHUNK];
        let mut acc = 0.0f32;
        let mut tap_idx = 0usize;
        let mut idx = pos;
        while tap_idx < n {
            let chunk = (n - tap_idx).min(CHUNK);
            for k in 0..chunk {
                ti[k] = taps[tap_idx + k];
                di[k] = delay[idx];
                idx = if idx == 0 { n - 1 } else { idx - 1 };
            }
            acc += dot_f32(&di[..chunk], &ti[..chunk]);
            tap_idx += chunk;
        }
        acc
    } else {
        let mut acc = 0.0f32;
        let mut idx = pos;
        for &tap in taps {
            acc += delay[idx] * tap;
            idx = if idx == 0 { n - 1 } else { idx - 1 };
        }
        acc
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
    let cutoff = passband_cutoff_hz(bandwidth_hz, design.cutoff_frac);
    let num_taps = plan_num_taps(
        sample_rate,
        bandwidth_hz,
        design.cutoff_frac,
        design.deep_selectivity,
    );
    let m = (num_taps - 1) as f32;
    let fc = cutoff / sample_rate;
    let dolph_db = design
        .dolph_sidelobe_db
        .clamp(MIN_DOLPH_SIDELOBE_DB, MAX_DOLPH_SIDELOBE_DB);
    let beta = if design.window == WindowKind::DolphChebyshev {
        kaiser_beta_from_sidelobe(dolph_db)
    } else {
        design.kaiser_beta.clamp(MIN_KAISER_BETA, MAX_KAISER_BETA)
    };
    let win_kind = if design.window == WindowKind::DolphChebyshev {
        WindowKind::Kaiser
    } else {
        design.window
    };

    let mut taps = Vec::with_capacity(num_taps);
    let mut sum = 0.0f32;
    for k in 0..num_taps {
        let x = k as f32 - m / 2.0;
        let sinc = if x.abs() < 1e-6 {
            2.0 * fc
        } else {
            (2.0 * PI * fc * x).sin() / (PI * x)
        };
        let tap = sinc * window_value(win_kind, k, num_taps, beta);
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

/// Minimal-tap Gaussian lowpass for wideband IQ decimation (23 taps, fixed cost).
pub fn design_gaussian_lowpass_compact(sample_rate: f32, bandwidth_hz: f32) -> FirFilter {
    const TAPS: usize = 23;
    let cutoff = (bandwidth_hz * 0.5).max(100.0);
    let fc = cutoff / sample_rate;
    let m = (TAPS - 1) as f32;
    let mut taps = Vec::with_capacity(TAPS);
    let mut sum = 0.0f32;
    for k in 0..TAPS {
        let x = k as f32 - m / 2.0;
        let sinc = if x.abs() < 1e-6 {
            2.0 * fc
        } else {
            (2.0 * PI * fc * x).sin() / (PI * x)
        };
        let tap = sinc * window_value(WindowKind::Gaussian, k, TAPS, filter_plan::DEFAULT_KAISER_BETA);
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

/// Short inverse-sinc compensator for upstream boxcar/CIC passband droop.
/// Assumes nominal boxcar length N≈7 (typical SDR channel filter); see liquid-dsp
/// inverse-sinc article. Only applied when [`LowpassDesign::passband_flatten`] is true.
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
        WindowKind::DolphChebyshev => kaiser_window(kf, m, kaiser_beta),
    }
}

/// Map target sidelobe attenuation (dB) to an equivalent Kaiser β.
fn kaiser_beta_from_sidelobe(sidelobe_db: f32) -> f32 {
    let a = sidelobe_db.clamp(MIN_DOLPH_SIDELOBE_DB, MAX_DOLPH_SIDELOBE_DB);
    let beta = if a > 50.0 {
        0.1102 * (a - 8.7)
    } else {
        0.5842 * (a - 21.0).powf(0.4) + 0.07886 * (a - 21.0)
    };
    beta.clamp(MIN_KAISER_BETA, MAX_KAISER_BETA)
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
    use super::filter_plan::CHANNEL_PASSBAND_MIN_HZ;
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
    fn block_fir_matches_scalar_path() {
        let rate = 12_000.0;
        let taps = design_lowpass_with(
            rate,
            200.0,
            LowpassDesign::default(),
        );
        let n = 2048;
        let input: Vec<Complex32> = (0..n)
            .map(|i| {
                let t = i as f32 / rate;
                let p = TAU * 200.0 * t;
                Complex32::new(p.cos(), p.sin())
            })
            .collect();

        let mut scalar = FirFilter::new(taps.taps().to_vec());
        let mut scalar_out = Vec::new();
        for &s in &input {
            scalar_out.push(scalar.process_complex(s));
        }

        let mut block = FirFilter::new(taps.taps().to_vec());
        let mut block_out = Vec::new();
        block.process_complex_block(&input, &mut block_out);
        assert_eq!(scalar_out.len(), block_out.len());

        let err: f32 = scalar_out
            .iter()
            .zip(block_out.iter())
            .map(|(a, b)| (a.re - b.re).abs() + (a.im - b.im).abs())
            .sum();
        assert!(err < 0.05, "block/scalar FIR mismatch err={err}");
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
                ..Default::default()
            },
            200.0,
            600.0,
        );
        assert!(db < -45.0, "Kaiser β=10 stopband only {db} dB");
    }

    #[test]
    fn dolph_passes_dc() {
        let rate = 12_000.0;
        let mut fir = design_lowpass_with(
            rate,
            200.0,
            LowpassDesign {
                window: WindowKind::DolphChebyshev,
                dolph_sidelobe_db: 60.0,
                ..Default::default()
            },
        );
        let mut peak = 0.0f32;
        for _ in 0..rate as usize {
            peak = peak.max(fir.process_complex(Complex32 { re: 1.0, im: 0.0 }).re.abs());
        }
        assert!(peak > 0.9, "DC gain too low: {peak}");
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
    fn gaussian_25hz_rejects_adjacent() {
        let db = stopband_db(LowpassDesign::default(), CHANNEL_PASSBAND_MIN_HZ, 75.0);
        assert!(db < -30.0, "75 Hz offset with 25 Hz BW: {db} dB");
    }

    #[test]
    fn blackman_25hz_rejects_close_adjacent() {
        let db = stopband_db(
            LowpassDesign {
                window: WindowKind::Blackman,
                ..Default::default()
            },
            CHANNEL_PASSBAND_MIN_HZ,
            60.0,
        );
        assert!(db < -35.0, "60 Hz offset with 25 Hz BW Blackman: {db} dB");
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

    #[test]
    fn skirt_rejection_outside_cyan_edge() {
        let bw = 200.0;
        let probe = bw * 0.6;
        let gauss = stopband_db(LowpassDesign::default(), bw, probe);
        let blackman = stopband_db(
            LowpassDesign {
                window: WindowKind::Blackman,
                ..Default::default()
            },
            bw,
            probe,
        );
        assert!(
            gauss < -24.0,
            "Gaussian skirt outside cyan: {gauss} dB"
        );
        assert!(
            blackman < -32.0,
            "Blackman skirt at cyan edge: {blackman} dB"
        );
    }
}
