use std::fmt;

use hfsdr::{DecimFilterKind, IqSource, KiwiSource};
#[cfg(feature = "airspy")]
use hfsdr::{airspyhf::iq_ring_capacity, AirspyHf};
#[cfg(feature = "qmx")]
use hfsdr::{
    qmx::{self, iq_ring_capacity as qmx_ring_capacity},
    QmxSource,
};
#[cfg(feature = "rtlsdr")]
use hfsdr::{
    rtlsdr::iq_ring_capacity as rtlsdr_ring_capacity,
    RtlSdr,
};
#[cfg(feature = "soapy")]
use hfsdr::{
    soapy::iq_ring_capacity as soapy_ring_capacity,
    SoapySource,
};
use serde::{Deserialize, Serialize};

use super::device::DeviceSource;
use super::iq_bridge::attach_dual_ring;
use super::settings::{AirspySettings, KiwiSettings, QmxSettings, RtlSdrSettings};
#[cfg(feature = "soapy")]
use super::settings::{default_soapy_sample_rate, SoapySettings};
#[cfg(feature = "airspy")]
use super::settings::default_airspy_sample_rate;
#[cfg(feature = "rtlsdr")]
use super::settings::default_rtlsdr_sample_rate;

/// Which front end to bring up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    #[cfg(feature = "airspy")]
    Airspy,
    #[cfg(feature = "rtlsdr")]
    RtlSdr,
    #[cfg(feature = "qmx")]
    Qmx,
    #[cfg(feature = "soapy")]
    Soapy,
    Kiwi,
}

impl fmt::Display for SourceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            #[cfg(feature = "airspy")]
            SourceKind::Airspy => write!(f, "Airspy HF+"),
            #[cfg(feature = "rtlsdr")]
            SourceKind::RtlSdr => write!(f, "RTL-SDR"),
            #[cfg(feature = "qmx")]
            SourceKind::Qmx => write!(f, "QMX"),
            #[cfg(feature = "soapy")]
            SourceKind::Soapy => write!(f, "SoapySDR"),
            SourceKind::Kiwi => write!(f, "KiwiSDR"),
        }
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
            #[cfg(feature = "rtlsdr")]
            "RtlSdr" => SourceKind::RtlSdr,
            #[cfg(feature = "qmx")]
            "Qmx" => SourceKind::Qmx,
            #[cfg(feature = "soapy")]
            "Soapy" => SourceKind::Soapy,
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
            #[cfg(feature = "rtlsdr")]
            SourceKind::RtlSdr => "RtlSdr",
            #[cfg(feature = "qmx")]
            SourceKind::Qmx => "Qmx",
            #[cfg(feature = "soapy")]
            SourceKind::Soapy => "Soapy",
            SourceKind::Kiwi => "Kiwi",
        };
        serializer.serialize_str(name)
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
    /// Airspy HF AGC, attenuator, LNA, and optional IQ decimation (ignored for Kiwi / RTL-SDR).
    #[serde(default)]
    pub airspy: AirspySettings,
    /// RTL-SDR gain, ppm, direct sampling, and optional IQ decimation (ignored for Kiwi / Airspy).
    #[serde(default)]
    pub rtlsdr: RtlSdrSettings,
    /// QMX CAT port, audio device, IF offset, and optional IQ decimation.
    #[serde(default)]
    pub qmx: QmxSettings,
    /// SoapySDR driver, device args, gain, and optional IQ decimation.
    #[cfg(feature = "soapy")]
    #[serde(default)]
    pub soapy: SoapySettings,
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
            rtlsdr: RtlSdrSettings::default(),
            qmx: QmxSettings::default(),
            #[cfg(feature = "soapy")]
            soapy: SoapySettings::default(),
        }
    }
}

impl ConnectRequest {
    /// A short label for the recent-hosts list / status bar.
    pub fn label(&self) -> String {
        match self.kind {
            #[cfg(feature = "airspy")]
            SourceKind::Airspy => format!("Airspy @ {:.3} MHz", self.center_hz / 1e6),
            #[cfg(feature = "rtlsdr")]
            SourceKind::RtlSdr => format!(
                "RTL-SDR #{} @ {:.3} MHz",
                self.rtlsdr.device_index,
                self.center_hz / 1e6
            ),
            #[cfg(feature = "qmx")]
            SourceKind::Qmx => format!("QMX @ {:.3} MHz", self.center_hz / 1e6),
            #[cfg(feature = "soapy")]
            SourceKind::Soapy => {
                let tag = self.soapy.device_args.trim();
                if tag.is_empty() {
                    format!("SoapySDR @ {:.3} MHz", self.center_hz / 1e6)
                } else {
                    let short = if tag.chars().count() <= 28 {
                        tag.to_string()
                    } else {
                        let keep = 13;
                        let head: String = tag.chars().take(keep).collect();
                        let tail: String = tag
                            .chars()
                            .rev()
                            .take(keep)
                            .collect::<Vec<_>>()
                            .into_iter()
                            .rev()
                            .collect();
                        format!("{head}…{tail}")
                    };
                    format!("Soapy {short} @ {:.3} MHz", self.center_hz / 1e6)
                }
            }
            SourceKind::Kiwi => format!("{}:{}", self.host, self.port),
        }
    }
}

/// Build, tune, and start the requested source. Blocks until the link is up
/// (or fails); intended to be called from the engine thread, never the UI.
/// Polls `cancel` during network setup so Disconnect/Cancel can abort promptly.
pub fn connect(req: &ConnectRequest, cancel: &std::sync::atomic::AtomicBool) -> Result<super::device::Connection, String> {
    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        return Err("connection cancelled".to_string());
    }
    if !super::kinds::source_kind_available(req.kind) {
        return Err(format!(
            "{} unavailable: native driver library not found (KiwiSDR and QMX still work)",
            req.kind
        ));
    }
    match req.kind {
        SourceKind::Kiwi => connect_kiwi(req, cancel),
        #[cfg(feature = "airspy")]
        SourceKind::Airspy => connect_airspy(req, cancel),
        #[cfg(feature = "rtlsdr")]
        SourceKind::RtlSdr => connect_rtlsdr(req, cancel),
        #[cfg(feature = "qmx")]
        SourceKind::Qmx => connect_qmx(req, cancel),
        #[cfg(feature = "soapy")]
        SourceKind::Soapy => connect_soapy(req, cancel),
    }
}

fn connect_kiwi(
    req: &ConnectRequest,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<super::device::Connection, String> {
    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        return Err("connection cancelled".to_string());
    }
    if req.host.trim().is_empty() {
        return Err("KiwiSDR host is empty".to_string());
    }
    let half = req.kiwi.passband_half_hz();
    let mut src = KiwiSource::new(req.host.clone(), req.port)
        .with_passband(-half, half)
        .with_freq_offset_khz(req.kiwi.freq_offset_khz)
        .with_ar_out_hz(req.kiwi.ar_out_hz)
        .with_agc(req.kiwi.rf_agc_on)
        .with_man_gain(req.kiwi.man_gain)
        .with_gen_attn(req.kiwi.gen_attn)
        .with_rf_attn_db(req.kiwi.rf_attn_db);
    src.tune(req.center_hz).map_err(|e| e.to_string())?;
    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        return Err("connection cancelled".to_string());
    }
    let reported = src.sample_rate();
    let (ingress_decim, eff_sr) = req.kiwi.ingress_decimation(reported);
    let device_iq = src.start_cancellable(cancel).map_err(|e| e.to_string())?;
    let ring_cap = 1 << 16;
    let (iq, iq_spectrum, bridge, iq_spectrum_ring_capacity) =
        attach_dual_ring(device_iq, ingress_decim, reported as f32, ring_cap, DecimFilterKind::LinearFir);
    Ok(super::device::Connection {
        device: DeviceSource::Kiwi(src),
        iq,
        iq_spectrum,
        bridge,
        iq_ring_capacity: ring_cap,
        iq_spectrum_ring_capacity,
        device_sample_rate: reported as f32,
        sample_rate: eff_sr,
        center_hz: req.center_hz,
        is_kiwi: true,
        iq_ingress_decim: ingress_decim,
    })
}

#[cfg(feature = "airspy")]
fn connect_airspy(
    req: &ConnectRequest,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<super::device::Connection, String> {
    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        return Err("connection cancelled".to_string());
    }
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
    let device_iq = src.start().map_err(|e| e.to_string())?;
    let (iq, iq_spectrum, bridge, iq_spectrum_ring_capacity) =
        attach_dual_ring(device_iq, ingress_decim, sr as f32, ring_cap, DecimFilterKind::LinearFir);
    Ok(super::device::Connection {
        device: DeviceSource::Airspy(src),
        iq,
        iq_spectrum,
        bridge,
        iq_ring_capacity: ring_cap,
        iq_spectrum_ring_capacity,
        device_sample_rate: sr as f32,
        sample_rate: eff_sr,
        center_hz: req.center_hz,
        is_kiwi: false,
        iq_ingress_decim: ingress_decim,
    })
}

#[cfg(feature = "rtlsdr")]
fn connect_rtlsdr(
    req: &ConnectRequest,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<super::device::Connection, String> {
    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        return Err("connection cancelled".to_string());
    }
    let mut src = RtlSdr::open_index(req.rtlsdr.device_index).map_err(|e| e.to_string())?;
    let sr = if req.sample_rate != 0 {
        req.sample_rate
    } else {
        default_rtlsdr_sample_rate()
    };
    src.set_sample_rate(sr).map_err(|e| e.to_string())?;
    if req.rtlsdr.ppm != 0 {
        src.set_freq_correction(req.rtlsdr.ppm).ok();
    }
    src.set_direct_sampling(req.rtlsdr.direct_sampling)
        .map_err(|e| e.to_string())?;
    src.set_offset_tuning(req.rtlsdr.offset_tuning).ok();
    src.set_rtl_agc(req.rtlsdr.rtl_agc).ok();
    src.set_tuner_gain_mode(req.rtlsdr.manual_gain)
        .map_err(|e| e.to_string())?;
    if req.rtlsdr.manual_gain {
        let gain = src.clamp_tuner_gain(req.rtlsdr.tuner_gain_db10);
        src.set_tuner_gain(gain).ok();
    }
    src.set_bias_tee(req.rtlsdr.bias_tee).ok();
    src.tune(req.center_hz).map_err(|e| e.to_string())?;
    let (ingress_decim, eff_sr) = req.rtlsdr.ingress_decimation(sr);
    let ring_cap = rtlsdr_ring_capacity(sr);
    let device_iq = src.start().map_err(|e| e.to_string())?;
    let (iq, iq_spectrum, bridge, iq_spectrum_ring_capacity) =
        attach_dual_ring(device_iq, ingress_decim, sr as f32, ring_cap, DecimFilterKind::LinearFir);
    Ok(super::device::Connection {
        device: DeviceSource::RtlSdr(src),
        iq,
        iq_spectrum,
        bridge,
        iq_ring_capacity: ring_cap,
        iq_spectrum_ring_capacity,
        device_sample_rate: sr as f32,
        sample_rate: eff_sr,
        center_hz: req.center_hz,
        is_kiwi: false,
        iq_ingress_decim: ingress_decim,
    })
}

#[cfg(feature = "qmx")]
fn connect_qmx(
    req: &ConnectRequest,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<super::device::Connection, String> {
    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        return Err("connection cancelled".to_string());
    }
    let q = &req.qmx;
    let mut src = QmxSource::open(
        &q.serial_port,
        &q.audio_device,
        q.if_offset_hz,
        q.rf_gain_db,
        q.disable_cat_timeout,
        q.force_cw_mode,
    )
    .map_err(|e| e.to_string())?;
    src.tune(req.center_hz).map_err(|e| e.to_string())?;
    let sr = qmx::SAMPLE_RATE;
    let (ingress_decim, eff_sr) = q.ingress_decimation(sr);
    let ring_cap = qmx_ring_capacity();
    let device_iq = src.start().map_err(|e| e.to_string())?;
    let (iq, iq_spectrum, bridge, iq_spectrum_ring_capacity) =
        attach_dual_ring(device_iq, ingress_decim, sr as f32, ring_cap, DecimFilterKind::LinearFir);
    Ok(super::device::Connection {
        device: DeviceSource::Qmx(src),
        iq,
        iq_spectrum,
        bridge,
        iq_ring_capacity: ring_cap,
        iq_spectrum_ring_capacity,
        device_sample_rate: sr as f32,
        sample_rate: eff_sr,
        center_hz: req.center_hz,
        is_kiwi: false,
        iq_ingress_decim: ingress_decim,
    })
}

#[cfg(feature = "soapy")]
fn connect_soapy(
    req: &ConnectRequest,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<super::device::Connection, String> {
    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        return Err("connection cancelled".to_string());
    }
    let args = req.soapy.device_args.trim();
    if args.is_empty() {
        return Err("SoapySDR device not selected — pick a device or enter args".to_string());
    }
    let mut src = SoapySource::open(args).map_err(|e| {
        format!("SoapySDR open failed: {e} ({})", hfsdr::soapy::last_error())
    })?;
    hfsdr::log::info(format!(
        "SoapySDR: opened {} (driver={})",
        args,
        src.driver()
    ));
    let rates = src.sample_rates();
    let requested = if req.sample_rate != 0 {
        req.sample_rate
    } else {
        default_soapy_sample_rate(&rates)
    };
    let sr = hfsdr::soapy::snap_sample_rate(requested, &rates);
    src.set_sample_rate(sr).map_err(|e| e.to_string())?;
    let device_rate = src.sample_rate();
    if !req.soapy.antenna.is_empty() {
        if let Err(e) = src.set_antenna_name(&req.soapy.antenna) {
            hfsdr::log::warn(format!(
                "SoapySDR: antenna '{}' not applied: {e}",
                req.soapy.antenna
            ));
        }
    }
    src.set_automatic_gain(req.soapy.agc).map_err(|e| e.to_string())?;
    if !req.soapy.agc {
        if let Err(e) = src.set_overall_gain(req.soapy.gain_db) {
            hfsdr::log::warn(format!(
                "SoapySDR: gain {:.1} dB not applied: {e}",
                req.soapy.gain_db
            ));
        }
    }
    src.tune(req.center_hz).map_err(|e| e.to_string())?;
    let (ingress_decim, eff_sr) = req.soapy.ingress_decimation(device_rate);
    let ring_cap = soapy_ring_capacity(device_rate);
    let device_iq = src.start().map_err(|e| e.to_string())?;
    let (iq, iq_spectrum, bridge, iq_spectrum_ring_capacity) =
        attach_dual_ring(device_iq, ingress_decim, device_rate as f32, ring_cap, DecimFilterKind::LinearFir);
    Ok(super::device::Connection {
        device: DeviceSource::Soapy(src),
        iq,
        iq_spectrum,
        bridge,
        iq_ring_capacity: ring_cap,
        iq_spectrum_ring_capacity,
        device_sample_rate: device_rate as f32,
        sample_rate: eff_sr,
        center_hz: req.center_hz,
        is_kiwi: false,
        iq_ingress_decim: ingress_decim,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn kiwi_label_includes_host_and_port() {
        let req = ConnectRequest {
            kind: SourceKind::Kiwi,
            host: "rx.example".into(),
            port: 8073,
            center_hz: 14_010_000.0,
            ..ConnectRequest::default()
        };
        assert_eq!(req.label(), "rx.example:8073");
    }

    #[test]
    fn connect_aborts_when_cancelled_before_start() {
        let cancel = AtomicBool::new(true);
        let req = ConnectRequest {
            kind: SourceKind::Kiwi,
            host: "unused.example".into(),
            port: 8073,
            center_hz: 14_010_000.0,
            ..ConnectRequest::default()
        };
        let err = connect(&req, &cancel).err().expect("cancelled");
        assert!(err.contains("cancelled"));
        cancel.store(false, Ordering::Relaxed);
    }

    #[test]
    fn connect_kiwi_rejects_empty_host() {
        let cancel = AtomicBool::new(false);
        let req = ConnectRequest {
            kind: SourceKind::Kiwi,
            host: "  ".into(),
            port: 8073,
            center_hz: 14_010_000.0,
            ..ConnectRequest::default()
        };
        let err = connect(&req, &cancel).err().expect("empty host");
        assert!(err.contains("empty"));
    }

    #[test]
    fn connect_request_default_is_kiwi() {
        let req = ConnectRequest::default();
        assert_eq!(req.kind, SourceKind::Kiwi);
        assert_eq!(req.port, 8073);
    }

    #[test]
    fn source_kind_serde_roundtrip() {
        let mut kinds = vec![SourceKind::Kiwi];
        #[cfg(feature = "airspy")]
        kinds.push(SourceKind::Airspy);
        #[cfg(feature = "rtlsdr")]
        kinds.push(SourceKind::RtlSdr);
        #[cfg(feature = "qmx")]
        kinds.push(SourceKind::Qmx);
        #[cfg(feature = "soapy")]
        kinds.push(SourceKind::Soapy);
        for kind in kinds {
            let req = ConnectRequest {
                kind,
                ..ConnectRequest::default()
            };
            let json = serde_json::to_string(&req).expect("serialize");
            let back: ConnectRequest = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(back.kind, kind);
        }
    }

    #[test]
    fn source_kind_deserialize_unknown_falls_back_to_kiwi() {
        let json = r#"{"kind":"UnknownDevice","host":"x","port":1,"center_hz":1.0,"sample_rate":0,"kiwi":{},"airspy":{},"rtlsdr":{},"qmx":{}}"#;
        let req: ConnectRequest = serde_json::from_str(json).expect("parse");
        assert_eq!(req.kind, SourceKind::Kiwi);
    }

    #[cfg(feature = "airspy")]
    #[test]
    fn airspy_label_includes_frequency() {
        let req = ConnectRequest {
            kind: SourceKind::Airspy,
            center_hz: 14_010_000.0,
            ..ConnectRequest::default()
        };
        assert!(req.label().contains("Airspy"));
        assert!(req.label().contains("14.010"));
    }

    #[cfg(feature = "rtlsdr")]
    #[test]
    fn rtlsdr_label_includes_device_index() {
        let req = ConnectRequest {
            kind: SourceKind::RtlSdr,
            center_hz: 7_100_000.0,
            rtlsdr: RtlSdrSettings {
                device_index: 2,
                ..RtlSdrSettings::default()
            },
            ..ConnectRequest::default()
        };
        assert!(req.label().contains("RTL-SDR #2"));
    }

    #[cfg(feature = "qmx")]
    #[test]
    fn qmx_label_includes_frequency() {
        let req = ConnectRequest {
            kind: SourceKind::Qmx,
            center_hz: 14_200_000.0,
            ..ConnectRequest::default()
        };
        assert!(req.label().contains("QMX"));
    }

    #[cfg(feature = "soapy")]
    #[test]
    fn soapy_label_includes_device_args() {
        let req = ConnectRequest {
            kind: SourceKind::Soapy,
            center_hz: 7_030_000.0,
            soapy: SoapySettings {
                device_args: "driver=rtlsdr,serial=ABC".into(),
                ..SoapySettings::default()
            },
            ..ConnectRequest::default()
        };
        assert!(req.label().contains("Soapy"));
        assert!(req.label().contains("7.030"));
    }

    #[test]
    fn source_kind_display_strings() {
        assert!(SourceKind::Kiwi.to_string().contains("Kiwi"));
        #[cfg(feature = "airspy")]
        assert!(SourceKind::Airspy.to_string().contains("Airspy"));
        #[cfg(feature = "rtlsdr")]
        assert!(SourceKind::RtlSdr.to_string().contains("RTL"));
        #[cfg(feature = "qmx")]
        assert!(SourceKind::Qmx.to_string().contains("QMX"));
        #[cfg(feature = "soapy")]
        assert!(SourceKind::Soapy.to_string().contains("Soapy"));
    }
}
