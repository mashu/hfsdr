use super::connection::{ConnectRequest, SourceKind};
use super::settings::{AirspySettings, KiwiSettings, QmxSettings, RtlSdrSettings};

/// Parse CLI args into a connect request for auto-connect on launch.
///
/// `waterfall kiwi <host> [port] [center_hz]` or
/// `waterfall airspy [sample_rate_hz] [center_hz] [process_hz]` (requires `airspy` feature) or
/// `waterfall rtlsdr [sample_rate_hz] [center_hz] [process_hz]` (requires `rtlsdr` feature).
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
                rtlsdr: RtlSdrSettings::default(),
                qmx: QmxSettings::default(),
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
                rtlsdr: RtlSdrSettings::default(),
                qmx: QmxSettings::default(),
            })
        }
        #[cfg(feature = "rtlsdr")]
        Some("rtlsdr") => {
            let sample_rate = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
            let center_hz = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(14_010_000.0);
            let process_hz = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(48_000);
            let mut rtlsdr = RtlSdrSettings::default();
            rtlsdr.iq_process_hz = process_hz;
            Some(ConnectRequest {
                kind: SourceKind::RtlSdr,
                host: String::new(),
                port: 8073,
                center_hz,
                sample_rate,
                kiwi: KiwiSettings::default(),
                airspy: AirspySettings::default(),
                rtlsdr,
                qmx: QmxSettings::default(),
            })
        }
        #[cfg(feature = "qmx")]
        Some("qmx") => {
            let center_hz = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(14_010_000.0);
            let process_hz = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(0);
            let serial = args.get(4).cloned().unwrap_or_default();
            let mut qmx = QmxSettings::default();
            qmx.iq_process_hz = process_hz;
            qmx.serial_port = serial;
            Some(ConnectRequest {
                kind: SourceKind::Qmx,
                host: String::new(),
                port: 8073,
                center_hz,
                sample_rate: 0,
                kiwi: KiwiSettings::default(),
                airspy: AirspySettings::default(),
                rtlsdr: RtlSdrSettings::default(),
                qmx,
            })
        }
        _ => None,
    }
}
