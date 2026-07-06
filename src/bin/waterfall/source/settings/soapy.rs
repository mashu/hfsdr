use serde::{Deserialize, Serialize};

/// SoapySDR hardware and client-side processing options.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SoapySettings {
    /// Driver filter for device enumeration (e.g. `rtlsdr`, `airspyhf`); empty = all.
    pub driver: String,
    /// Full device args passed to `SoapySDRDevice_makeStrArgs` (e.g. `driver=rtlsdr,serial=…`).
    pub device_args: String,
    /// RX antenna name; empty = driver default.
    pub antenna: String,
    /// Overall gain in dB when manual gain mode is active.
    pub gain_db: f64,
    /// Hardware AGC / automatic gain mode.
    pub agc: bool,
    /// Client-side IQ decimation target in Hz; `0` = auto.
    pub iq_process_hz: u32,
}

impl Default for SoapySettings {
    fn default() -> Self {
        Self {
            driver: String::new(),
            device_args: String::new(),
            antenna: String::new(),
            gain_db: 30.0,
            agc: false,
            iq_process_hz: 0,
        }
    }
}

#[cfg(feature = "soapy")]
impl SoapySettings {
    /// Soapy drivers deliver IQ in bursts — keep full device rate unless the
    /// operator explicitly sets Process IQ (unlike native Airspy/RTL auto-decim).
    pub fn ingress_decimation(&self, device_rate: u32) -> (usize, f32) {
        if self.iq_process_hz == 0 {
            return (1, device_rate as f32);
        }
        hfsdr::ingress_decimation_from_hz(self.iq_process_hz, device_rate)
    }
}

/// Preferred default sample rate when the connect request leaves rate at `0`.
#[cfg(feature = "soapy")]
pub fn default_soapy_sample_rate(rates: &[u32]) -> u32 {
    hfsdr::soapy::default_sample_rate(rates)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soapy_default_process_is_native() {
        assert_eq!(SoapySettings::default().iq_process_hz, 0);
    }

    #[test]
    #[cfg(feature = "soapy")]
    fn soapy_default_ingress_stays_at_device_rate() {
        let s = SoapySettings::default();
        assert_eq!(s.ingress_decimation(768_000), (1, 768_000.0));
        assert_eq!(s.ingress_decimation(384_000), (1, 384_000.0));
    }

    #[test]
    #[cfg(feature = "soapy")]
    fn soapy_explicit_process_decimates() {
        let mut s = SoapySettings::default();
        s.iq_process_hz = 192_000;
        assert_eq!(s.ingress_decimation(768_000), (4, 192_000.0));
    }
}
