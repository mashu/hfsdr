//! Optional 2-pole IIR channel filter (I/Q parallel biquads).
//!
//! Steeper skirts than a short FIR but **non-linear phase** — can ring on keying
//! edges. Prefer [`super::fir::FirFilter`] for normal CW; this exists for A/B.

use std::f32::consts::FRAC_1_SQRT_2;

use crate::dsp::biquad::Biquad;
use crate::source::Complex32;

use super::settings::IirFilterKind;

/// Passband ripple for 2-pole Chebyshev Type I (dB).
pub const DEFAULT_IIR_CHEBYSHEV_RIPPLE_DB: f32 = 2.0;

/// Biquad Q for a 2nd-order lowpass prototype.
pub fn iir_2pole_lowpass_q(kind: IirFilterKind) -> f32 {
    match kind {
        IirFilterKind::Butterworth => FRAC_1_SQRT_2,
        IirFilterKind::Chebyshev => chebyshev1_2pole_q(DEFAULT_IIR_CHEBYSHEV_RIPPLE_DB),
    }
}

fn chebyshev1_2pole_q(ripple_db: f32) -> f32 {
    let rp = ripple_db.max(0.01);
    let eps = (10.0_f32.powf(rp / 10.0) - 1.0).sqrt();
    let nu = (1.0 / eps).asinh() * 0.5;
    1.0 / (2.0 * nu.sinh())
}

#[derive(Clone, Debug)]
pub struct IirChannelFilter {
    i: Biquad,
    q: Biquad,
    last_rate: f32,
    last_cutoff: f32,
    last_kind: Option<IirFilterKind>,
}

impl Default for IirChannelFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl IirChannelFilter {
    pub fn new() -> Self {
        Self {
            i: Biquad::new(),
            q: Biquad::new(),
            last_rate: 0.0,
            last_cutoff: 0.0,
            last_kind: None,
        }
    }

    pub fn sync(&mut self, sample_rate: f32, bandwidth_hz: f32, kind: IirFilterKind) {
        let cutoff = (bandwidth_hz * 0.5).max(10.0);
        if (sample_rate - self.last_rate).abs() > 1.0
            || (cutoff - self.last_cutoff).abs() > 1.0
            || self.last_kind != Some(kind)
        {
            let q = iir_2pole_lowpass_q(kind);
            self.i.set_lowpass(sample_rate, cutoff, q);
            self.q.set_lowpass(sample_rate, cutoff, q);
            self.last_rate = sample_rate;
            self.last_cutoff = cutoff;
            self.last_kind = Some(kind);
        }
    }

    pub fn reset_state(&mut self) {
        self.i.reset_state();
        self.q.reset_state();
    }

    pub fn process_complex(&mut self, sample: Complex32) -> Complex32 {
        Complex32 {
            re: self.i.process(sample.re),
            im: self.q.process(sample.im),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    #[test]
    fn lowpass_attenuates_high_tone() {
        let rate = 12_000.0;
        let bw = 200.0;

        fn tone_power(kind: IirFilterKind, rate: f32, bw: f32, tone_hz: f32) -> f32 {
            let mut filt = IirChannelFilter::new();
            filt.sync(rate, bw, kind);
            let mut pwr = 0.0f32;
            for i in 0..rate as usize {
                let t = i as f32 / rate;
                let s = Complex32::new((TAU * tone_hz * t).cos(), (TAU * tone_hz * t).sin());
                let o = filt.process_complex(s);
                if i > rate as usize / 2 {
                    pwr += o.norm_sqr();
                }
            }
            pwr
        }

        let lo_pwr = tone_power(IirFilterKind::Butterworth, rate, bw, 30.0);
        let hi_pwr = tone_power(IirFilterKind::Butterworth, rate, bw, 900.0);
        assert!(lo_pwr > hi_pwr * 2.0, "IIR should pass DC-near tone more than far tone");
    }

    #[test]
    fn chebyshev_passband_ripple_above_butterworth() {
        use crate::dsp::biquad::Biquad;

        let rate = 12_000.0_f32;
        let cutoff = 100.0;
        let test_hz = 60.0;

        let mut butter = Biquad::new();
        butter.set_lowpass(rate, cutoff, iir_2pole_lowpass_q(IirFilterKind::Butterworth));
        let mut cheby = Biquad::new();
        cheby.set_lowpass(rate, cutoff, iir_2pole_lowpass_q(IirFilterKind::Chebyshev));

        let m_butter = butter.magnitude_linear(rate, test_hz);
        let m_cheby = cheby.magnitude_linear(rate, test_hz);
        assert!(
            m_cheby > m_butter,
            "Chebyshev passband ripple should peak above Butterworth: butter={m_butter} cheby={m_cheby}"
        );
    }

    #[test]
    fn chebyshev_q_exceeds_butterworth() {
        assert!(iir_2pole_lowpass_q(IirFilterKind::Chebyshev)
            > iir_2pole_lowpass_q(IirFilterKind::Butterworth));
    }
}
