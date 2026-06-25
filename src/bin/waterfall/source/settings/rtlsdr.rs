use serde::{Deserialize, Serialize};

/// RTL-SDR hardware and client-side processing options.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RtlSdrSettings {
    /// USB device index (`0` = first dongle).
    pub device_index: u32,
    /// RTL2832 internal digital AGC.
    pub rtl_agc: bool,
    /// Manual tuner gain mode (when off, tuner AGC / auto gain applies).
    pub manual_gain: bool,
    /// Tuner gain in tenths of a dB (e.g. 290 = 29.0 dB); clamped at connect.
    pub tuner_gain_db10: i32,
    /// Frequency correction in parts-per-million.
    pub ppm: i32,
    /// Direct sampling: 0 = off, 1 = I-ADC, 2 = Q-ADC (HF via IF, 0–28.8 MHz).
    pub direct_sampling: u8,
    /// Offset tuning to move DC spur away from tune (zero-IF tuners).
    pub offset_tuning: bool,
    /// GPIO bias tee for active antennas / upconverters.
    pub bias_tee: bool,
    /// Client-side IQ decimation target in Hz; `0` = native device rate.
    pub iq_process_hz: u32,
}

impl Default for RtlSdrSettings {
    fn default() -> Self {
        Self {
            device_index: 0,
            rtl_agc: false,
            manual_gain: true,
            tuner_gain_db10: 290,
            ppm: 0,
            direct_sampling: 0,
            offset_tuning: false,
            bias_tee: false,
            iq_process_hz: 0,
        }
    }
}

#[cfg(feature = "rtlsdr")]
impl RtlSdrSettings {
    pub fn ingress_decimation(&self, device_rate: u32) -> (usize, f32) {
        if self.iq_process_hz == 0 || self.iq_process_hz >= device_rate {
            return (1, device_rate as f32);
        }
        if device_rate.is_multiple_of(self.iq_process_hz) {
            let factor = (device_rate / self.iq_process_hz) as usize;
            (factor.max(1), self.iq_process_hz as f32)
        } else {
            (1, device_rate as f32)
        }
    }
}

/// Preferred default RTL-SDR sample rate when the connect request leaves rate at `0`.
#[cfg(feature = "rtlsdr")]
pub fn default_rtlsdr_sample_rate() -> u32 {
    hfsdr::rtlsdr::DEFAULT_SAMPLE_RATE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rtlsdr_default_process_is_native() {
        assert_eq!(RtlSdrSettings::default().iq_process_hz, 0);
    }

    #[test]
    #[cfg(feature = "rtlsdr")]
    fn rtlsdr_ingress_decimation_divides_evenly() {
        let mut s = RtlSdrSettings::default();
        s.iq_process_hz = 48_000;
        assert_eq!(s.ingress_decimation(1_920_000), (40, 48_000.0));
    }
}
