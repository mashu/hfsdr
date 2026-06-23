//! Source description and construction for the waterfall binary.
//!
//! A [`ConnectRequest`] fully describes how to bring up a front end; [`connect`]
//! builds, tunes, and starts it. The request is created either from CLI args
//! (auto-connect on launch) or from the in-app connection form, and is the unit
//! we persist as a "recent host".

use std::fmt;

use hfsdr::{Complex32, Consumer, IqSource, kiwi_iq_half_hz, KiwiSource, KIWI_IQ_RATE};
#[cfg(feature = "airspy")]
use hfsdr::{AirspyHf, airspyhf::iq_ring_capacity};
use serde::{Deserialize, Serialize};

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
}

impl Default for KiwiSettings {
    fn default() -> Self {
        Self {
            iq_rate_hz: KIWI_IQ_RATE,
            iq_half_bw_hz: 0,
            iq_resample_hz: 0,
            freq_offset_khz: 0.0,
            ar_out_hz: 44_100,
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

fn default_hf_lna_on() -> bool {
    true
}

impl AirspySettings {
    pub fn frontend_flags(&self) -> u32 {
        let mut flags = 0u32;
        #[cfg(feature = "airspy")]
        {
            if self.frontend_optimize_band_iii {
                flags |= hfsdr::airspyhf::FLAGS_OPTIMIZE_BAND_III;
            }
            if self.frontend_optimize_pll_boundary {
                flags |= hfsdr::airspyhf::FLAGS_OPTIMIZE_PLL_INT_BOUNDARY;
            }
        }
        flags
    }

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

/// Which front end to bring up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    #[cfg(feature = "airspy")]
    Airspy,
    Kiwi,
}

impl fmt::Display for SourceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            #[cfg(feature = "airspy")]
            SourceKind::Airspy => write!(f, "Airspy HF+"),
            SourceKind::Kiwi => write!(f, "KiwiSDR"),
        }
    }
}

/// A fully-specified request to connect to a front end.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConnectRequest {
    pub kind: SourceKind,
    /// KiwiSDR host (ignored for Airspy).
    pub host: String,
    /// KiwiSDR port (ignored for Airspy).
    pub port: u16,
    pub center_hz: f64,
    /// Airspy sample rate in Hz; `0` selects [`default_airspy_sample_rate`] (ignored for Kiwi).
    pub sample_rate: u32,
    /// Kiwi IQ passband, resample, and transverter offset (ignored for Airspy).
    #[serde(default)]
    pub kiwi: KiwiSettings,
    /// Airspy HF AGC, attenuator, LNA, and optional IQ decimation (ignored for Kiwi).
    #[serde(default)]
    pub airspy: AirspySettings,
}

impl Default for ConnectRequest {
    fn default() -> Self {
        Self {
            kind: SourceKind::Kiwi,
            host: String::new(),
            port: 8073,
            center_hz: 14_010_000.0,
            sample_rate: 0,
            kiwi: KiwiSettings::default(),
            airspy: AirspySettings::default(),
        }
    }
}

impl ConnectRequest {
    /// A short label for the recent-hosts list / status bar.
    pub fn label(&self) -> String {
        match self.kind {
            #[cfg(feature = "airspy")]
            SourceKind::Airspy => format!("Airspy @ {:.3} MHz", self.center_hz / 1e6),
            SourceKind::Kiwi => format!("{}:{}", self.host, self.port),
        }
    }
}

/// A connected, streaming source: the boxed front end plus its IQ consumer.
pub struct Connection {
    pub source: Box<dyn IqSource>,
    pub iq: Consumer<Complex32>,
    pub iq_ring_capacity: usize,
    /// Native device IQ rate (full passband for demod and display).
    pub device_sample_rate: f32,
    /// Rate after optional client-side decimation (spectrum FFT path only).
    pub sample_rate: f32,
    pub center_hz: f64,
    pub is_kiwi: bool,
    /// Client-side integer decimation for the spectrum path (1 = none).
    pub iq_ingress_decim: usize,
}

/// Build, tune, and start the requested source. Blocks until the link is up
/// (or fails); intended to be called from the engine thread, never the UI.
pub fn connect(req: &ConnectRequest) -> Result<Connection, String> {
    match req.kind {
        SourceKind::Kiwi => connect_kiwi(req),
        #[cfg(feature = "airspy")]
        SourceKind::Airspy => connect_airspy(req),
    }
}

fn connect_kiwi(req: &ConnectRequest) -> Result<Connection, String> {
    if req.host.trim().is_empty() {
        return Err("KiwiSDR host is empty".to_string());
    }
    let half = req.kiwi.passband_half_hz();
    let mut src = KiwiSource::new(req.host.clone(), req.port)
        .with_passband(-half, half)
        .with_freq_offset_khz(req.kiwi.freq_offset_khz)
        .with_ar_out_hz(req.kiwi.ar_out_hz);
    src.tune(req.center_hz).map_err(|e| e.to_string())?;
    let reported = src.sample_rate();
    let (ingress_decim, eff_sr) = req.kiwi.ingress_decimation(reported);
    let iq = src.start().map_err(|e| e.to_string())?;
    Ok(Connection {
        source: Box::new(src),
        iq,
        iq_ring_capacity: 1 << 16,
        device_sample_rate: reported as f32,
        sample_rate: eff_sr,
        center_hz: req.center_hz,
        is_kiwi: true,
        iq_ingress_decim: ingress_decim,
    })
}

#[cfg(feature = "airspy")]
fn connect_airspy(req: &ConnectRequest) -> Result<Connection, String> {
    let mut src = AirspyHf::open().map_err(|e| e.to_string())?;
    let sr = if req.sample_rate != 0 {
        req.sample_rate
    } else {
        default_airspy_sample_rate(&src.sample_rates())
    };
    src.set_sample_rate(sr).map_err(|e| e.to_string())?;
    src.set_lib_dsp(req.airspy.lib_dsp).ok();
    if req.airspy.calibration_ppb != 0 {
        src.set_calibration_ppb(req.airspy.calibration_ppb).ok();
    }
    src.set_hf_agc(req.airspy.hf_agc).map_err(|e| e.to_string())?;
    src.set_hf_agc_threshold(req.airspy.hf_agc_threshold_high)
        .ok();
    src.set_hf_att(req.airspy.hf_att).ok();
    src.set_hf_lna(req.airspy.hf_lna).ok();
    src.set_frontend_options(req.airspy.frontend_flags()).ok();
    src.set_bias_tee(req.airspy.bias_tee).ok();
    src.tune(req.center_hz).map_err(|e| e.to_string())?;
    let (ingress_decim, eff_sr) = req.airspy.ingress_decimation(sr);
    let ring_cap = iq_ring_capacity(sr);
    let iq = src.start().map_err(|e| e.to_string())?;
    Ok(Connection {
        source: Box::new(src),
        iq,
        iq_ring_capacity: ring_cap,
        device_sample_rate: sr as f32,
        sample_rate: eff_sr,
        center_hz: req.center_hz,
        is_kiwi: false,
        iq_ingress_decim: ingress_decim,
    })
}

/// Parse CLI args into a connect request for auto-connect on launch.
///
/// `waterfall kiwi <host> [port] [center_hz]` or
/// `waterfall airspy [sample_rate_hz] [center_hz] [process_hz]` (requires `airspy` feature).
pub fn request_from_args() -> Option<ConnectRequest> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("kiwi") => {
            let host = args.get(2).cloned()?;
            let port = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(8073);
            let center_hz = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(14_010_000.0);
            Some(ConnectRequest {
                kind: SourceKind::Kiwi,
                host,
                port,
                center_hz,
                sample_rate: 0,
                kiwi: KiwiSettings::default(),
                airspy: AirspySettings::default(),
            })
        }
        #[cfg(feature = "airspy")]
        Some("airspy") => {
            let sample_rate = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
            let center_hz = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(14_010_000.0);
            let process_hz = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(48_000);
            let mut airspy = AirspySettings::default();
            airspy.iq_process_hz = process_hz;
            Some(ConnectRequest {
                kind: SourceKind::Airspy,
                host: String::new(),
                port: 8073,
                center_hz,
                sample_rate,
                kiwi: KiwiSettings::default(),
                airspy,
            })
        }
        _ => None,
    }
}

/// Deserialize a persisted [`SourceKind`]; unknown variants fall back to Kiwi.
impl<'de> Deserialize<'de> for SourceKind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(match s.as_str() {
            #[cfg(feature = "airspy")]
            "Airspy" => SourceKind::Airspy,
            _ => SourceKind::Kiwi,
        })
    }
}

impl Serialize for SourceKind {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let name = match self {
            #[cfg(feature = "airspy")]
            SourceKind::Airspy => "Airspy",
            SourceKind::Kiwi => "Kiwi",
        };
        serializer.serialize_str(name)
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
    fn kiwi_ingress_decimation_divides_evenly() {
        let mut s = KiwiSettings::default();
        s.iq_resample_hz = 6_000;
        assert_eq!(s.ingress_decimation(12_000), (2, 6_000.0));
    }

    #[test]
    fn airspy_default_preamp_on() {
        assert!(AirspySettings::default().hf_lna);
    }

    #[test]
    fn airspy_default_process_is_native() {
        assert_eq!(AirspySettings::default().iq_process_hz, 0);
    }

    #[test]
    fn airspy_ingress_decimation_divides_evenly() {
        let mut s = AirspySettings::default();
        s.iq_process_hz = 192_000;
        assert_eq!(s.ingress_decimation(768_000), (4, 192_000.0));
    }

    #[test]
    #[cfg(feature = "airspy")]
    fn airspy_default_rate_prefers_384k() {
        let rates = vec![3_000, 192_000, 384_000, 768_000];
        assert_eq!(default_airspy_sample_rate(&rates), 384_000);
    }
}
