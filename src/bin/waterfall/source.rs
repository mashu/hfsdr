//! Source description and construction for the waterfall binary.
//!
//! A [`ConnectRequest`] fully describes how to bring up a front end; [`connect`]
//! builds, tunes, and starts it. The request is created either from CLI args
//! (auto-connect on launch) or from the in-app connection form, and is the unit
//! we persist as a "recent host".

use std::fmt;

use hfsdr::{Complex32, Consumer, IqSource, KiwiSource, KIWI_IQ_HALF_HZ};
#[cfg(feature = "airspy")]
use hfsdr::AirspyHf;
use serde::{Deserialize, Serialize};

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
    /// Airspy sample rate; `0` selects the device default (ignored for Kiwi).
    pub sample_rate: u32,
}

impl Default for ConnectRequest {
    fn default() -> Self {
        Self {
            kind: SourceKind::Kiwi,
            host: String::new(),
            port: 8073,
            center_hz: 14_010_000.0,
            sample_rate: 0,
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
    pub sample_rate: f32,
    pub center_hz: f64,
    pub is_kiwi: bool,
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
    let mut src = KiwiSource::new(req.host.clone(), req.port)
        .with_passband(-KIWI_IQ_HALF_HZ, KIWI_IQ_HALF_HZ);
    src.tune(req.center_hz).map_err(|e| e.to_string())?;
    let sr = src.sample_rate() as f32;
    let iq = src.start().map_err(|e| e.to_string())?;
    Ok(Connection {
        source: Box::new(src),
        iq,
        iq_ring_capacity: 1 << 16,
        sample_rate: sr,
        center_hz: req.center_hz,
        is_kiwi: true,
    })
}

#[cfg(feature = "airspy")]
fn connect_airspy(req: &ConnectRequest) -> Result<Connection, String> {
    let mut src = AirspyHf::open().map_err(|e| e.to_string())?;
    let sr = if req.sample_rate != 0 {
        req.sample_rate
    } else {
        src.sample_rates().first().copied().unwrap_or(768_000)
    };
    src.set_sample_rate(sr).map_err(|e| e.to_string())?;
    src.set_lib_dsp(true).ok();
    src.tune(req.center_hz).map_err(|e| e.to_string())?;
    let iq = src.start().map_err(|e| e.to_string())?;
    Ok(Connection {
        source: Box::new(src),
        iq,
        iq_ring_capacity: 1 << 15,
        sample_rate: sr as f32,
        center_hz: req.center_hz,
        is_kiwi: false,
    })
}

/// Parse CLI args into a connect request for auto-connect on launch.
///
/// `waterfall kiwi <host> [port] [center_hz]` or
/// `waterfall airspy [sample_rate_hz] [center_hz]` (requires `airspy` feature).
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
            })
        }
        #[cfg(feature = "airspy")]
        Some("airspy") => {
            let sample_rate = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
            let center_hz = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(14_010_000.0);
            Some(ConnectRequest {
                kind: SourceKind::Airspy,
                host: String::new(),
                port: 8073,
                center_hz,
                sample_rate,
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
