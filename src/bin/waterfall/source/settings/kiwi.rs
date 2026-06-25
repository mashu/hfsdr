use hfsdr::{kiwi_iq_half_hz, KIWI_IQ_RATE};
use serde::{Deserialize, Serialize};

fn default_true() -> bool {
    true
}

fn default_kiwi_man_gain() -> u8 {
    hfsdr::kiwi::protocol::KIWI_MAN_GAIN_DEFAULT
}

/// Kiwi IQ stream options sent at connect (see kiwiclient `-L`/`-H`/`-o`/`-r`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct KiwiSettings {
    /// Expected Kiwi IQ rate in Hz (caps passband; server reports actual rate on connect).
    pub iq_rate_hz: u32,
    /// IQ half-bandwidth in Hz; `0` = maximum for [`iq_rate_hz`] (rate/2 − 20).
    pub iq_half_bw_hz: u32,
    /// Client-side IQ resample target in Hz; `0` = native server rate.
    pub iq_resample_hz: u32,
    /// Frequency offset in kHz subtracted from the displayed tune frequency (kiwiclient `-o`).
    pub freq_offset_khz: f64,
    /// `SET AR OK out=` audio resampler output rate.
    pub ar_out_hz: u32,
    /// RF gain as Kiwi `manGain` 0..=100 (−100..0 dB below max). Active only when RF AGC is off.
    #[serde(default = "default_kiwi_man_gain")]
    pub man_gain: u8,
    /// Test generator attenuation (`SET genattn=` during IQ handshake).
    #[serde(default)]
    pub gen_attn: u8,
    /// Hardware RF attenuator in dB (KiwiSDR 2 when `has_attn=1`).
    #[serde(default)]
    pub rf_attn_db: f32,
    /// Kiwi hardware RF AGC (`SET agc=`); persisted with recent hosts.
    #[serde(default = "default_true")]
    pub rf_agc_on: bool,
}

impl Default for KiwiSettings {
    fn default() -> Self {
        Self {
            iq_rate_hz: KIWI_IQ_RATE,
            iq_half_bw_hz: 0,
            iq_resample_hz: 0,
            freq_offset_khz: 0.0,
            ar_out_hz: 44_100,
            man_gain: default_kiwi_man_gain(),
            gen_attn: 0,
            rf_attn_db: 0.0,
            rf_agc_on: true,
        }
    }
}

impl KiwiSettings {
    pub fn passband_half_hz(&self) -> i32 {
        let max = kiwi_iq_half_hz(self.iq_rate_hz.max(1_000));
        if self.iq_half_bw_hz == 0 {
            max
        } else {
            (self.iq_half_bw_hz as i32).clamp(500, max)
        }
    }

    pub fn ingress_decimation(&self, reported_rate: u32) -> (usize, f32) {
        if self.iq_resample_hz == 0 || self.iq_resample_hz >= reported_rate {
            return (1, reported_rate as f32);
        }
        if reported_rate.is_multiple_of(self.iq_resample_hz) {
            let factor = (reported_rate / self.iq_resample_hz) as usize;
            (factor.max(1), self.iq_resample_hz as f32)
        } else {
            (1, reported_rate as f32)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kiwi_default_passband_is_max() {
        let s = KiwiSettings::default();
        assert_eq!(s.passband_half_hz(), 5_980);
    }

    #[test]
    fn kiwi_default_man_gain_is_full_scale() {
        let s = KiwiSettings::default();
        assert_eq!(s.man_gain, hfsdr::kiwi::protocol::KIWI_MAN_GAIN_DEFAULT);
        assert_eq!(s.man_gain, 100);
    }

    #[test]
    fn kiwi_ingress_decimation_divides_evenly() {
        let mut s = KiwiSettings::default();
        s.iq_resample_hz = 6_000;
        assert_eq!(s.ingress_decimation(12_000), (2, 6_000.0));
    }
}
