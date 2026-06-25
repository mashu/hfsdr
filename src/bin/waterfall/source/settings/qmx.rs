use serde::{Deserialize, Serialize};

/// QMX / QMX+ CAT and USB-audio IQ options.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct QmxSettings {
    /// Virtual COM port path (empty = first available port).
    pub serial_port: String,
    /// USB sound card input name (empty = auto-detect QMX/QRP device).
    pub audio_device: String,
    /// Superhet IF offset applied when tuning (Hz subtracted from VFO `FA` command).
    pub if_offset_hz: i32,
    /// RF gain in dB (CAT `RG` command, band-dependent maximum).
    pub rf_gain_db: u8,
    /// Disable CAT TX timeout so the radio stays in RX during SDR use.
    pub disable_cat_timeout: bool,
    /// Force CW operating mode at connect (recommended for CW skimming).
    pub force_cw_mode: bool,
    /// Client-side IQ decimation target in Hz; `0` = native 48 kHz.
    pub iq_process_hz: u32,
}

impl Default for QmxSettings {
    fn default() -> Self {
        Self {
            serial_port: String::new(),
            audio_device: String::new(),
            if_offset_hz: 12_000,
            rf_gain_db: 50,
            disable_cat_timeout: true,
            force_cw_mode: true,
            iq_process_hz: 0,
        }
    }
}

#[cfg(feature = "qmx")]
impl QmxSettings {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_if_offset_matches_qmx_superhet() {
        let s = QmxSettings::default();
        assert_eq!(s.if_offset_hz, 12_000);
        assert!(s.disable_cat_timeout);
        assert!(s.force_cw_mode);
    }

    #[cfg(feature = "qmx")]
    #[test]
    fn ingress_decimation_divides_evenly() {
        let s = QmxSettings {
            iq_process_hz: 12_000,
            ..Default::default()
        };
        let (factor, rate) = s.ingress_decimation(48_000);
        assert_eq!(factor, 4);
        assert!((rate - 12_000.0).abs() < 1.0);
    }

    #[cfg(feature = "qmx")]
    #[test]
    fn ingress_decimation_falls_back_when_not_divisible() {
        let s = QmxSettings {
            iq_process_hz: 10_000,
            ..Default::default()
        };
        let (factor, rate) = s.ingress_decimation(48_000);
        assert_eq!(factor, 1);
        assert!((rate - 48_000.0).abs() < 1.0);
    }
}
