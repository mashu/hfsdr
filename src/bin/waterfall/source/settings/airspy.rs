use serde::{Deserialize, Serialize};

fn default_hf_lna_on() -> bool {
    true
}

/// Airspy HF+ hardware and client-side processing options.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AirspySettings {
    /// HF AGC on/off (recommended on for most bands).
    pub hf_agc: bool,
    /// HF AGC threshold: `false` = low, `true` = high.
    pub hf_agc_threshold_high: bool,
    /// HF attenuator step 0..=8 (6 dB per step).
    pub hf_att: u8,
    /// LNA / preamp (+6 dB, compensated digitally). Enable for passive antennas.
    #[serde(default = "default_hf_lna_on")]
    pub hf_lna: bool,
    /// VHF Band-III frontend optimization (Discovery / Ranger).
    pub frontend_optimize_band_iii: bool,
    /// PLL integer-boundary optimization (Discovery / Ranger).
    pub frontend_optimize_pll_boundary: bool,
    /// Antenna-port bias tee — DC power for active preamps/upconverters.
    pub bias_tee: bool,
    /// Library IQ correction, IF shift, and fine tuning.
    pub lib_dsp: bool,
    /// Frequency calibration in parts-per-billion.
    pub calibration_ppb: i32,
    /// Client-side IQ decimation target in Hz; `0` = native device rate.
    pub iq_process_hz: u32,
}

impl Default for AirspySettings {
    fn default() -> Self {
        Self {
            hf_agc: true,
            hf_agc_threshold_high: false,
            hf_att: 0,
            hf_lna: true,
            frontend_optimize_band_iii: false,
            frontend_optimize_pll_boundary: false,
            bias_tee: false,
            lib_dsp: true,
            calibration_ppb: 0,
            iq_process_hz: 0,
        }
    }
}

#[cfg(feature = "airspy")]
impl AirspySettings {
    pub fn frontend_flags(&self) -> u32 {
        let mut flags = 0u32;
        if self.frontend_optimize_band_iii {
            flags |= hfsdr::airspyhf::FLAGS_OPTIMIZE_BAND_III;
        }
        if self.frontend_optimize_pll_boundary {
            flags |= hfsdr::airspyhf::FLAGS_OPTIMIZE_PLL_INT_BOUNDARY;
        }
        flags
    }

    pub fn ingress_decimation(&self, device_rate: u32) -> (usize, f32) {
        hfsdr::ingress_decimation_from_hz(self.iq_process_hz, device_rate)
    }
}

/// Preferred default sample rate when the connect request leaves rate at `0`.
#[cfg(feature = "airspy")]
pub fn default_airspy_sample_rate(rates: &[u32]) -> u32 {
    const PREFERRED: &[u32] = &[384_000, 192_000, 768_000, 96_000, 48_000];
    for &p in PREFERRED {
        if rates.contains(&p) {
            return p;
        }
    }
    rates.last().copied().unwrap_or(384_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn airspy_default_preamp_on() {
        assert!(AirspySettings::default().hf_lna);
    }

    #[test]
    fn airspy_default_process_is_native() {
        assert_eq!(AirspySettings::default().iq_process_hz, 0);
    }

    #[test]
    #[cfg(feature = "airspy")]
    fn airspy_ingress_decimation_divides_evenly() {
        let mut s = AirspySettings::default();
        s.iq_process_hz = 192_000;
        assert_eq!(s.ingress_decimation(768_000), (4, 192_000.0));
    }

    #[test]
    #[cfg(feature = "airspy")]
    fn airspy_auto_process_at_768k_when_unset() {
        assert_eq!(AirspySettings::default().ingress_decimation(768_000), (4, 192_000.0));
    }

    #[test]
    #[cfg(feature = "airspy")]
    fn airspy_default_rate_prefers_384k() {
        let rates = vec![3_000, 192_000, 384_000, 768_000];
        assert_eq!(default_airspy_sample_rate(&rates), 384_000);
    }
}
