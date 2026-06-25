use super::connection::{ConnectRequest, SourceKind};
use super::settings::{AirspySettings, KiwiSettings, QmxSettings, RtlSdrSettings};

/// Parse CLI args into a connect request for auto-connect on launch.
///
/// `waterfall kiwi <host> [port] [center_hz]` or
/// `waterfall airspy [sample_rate_hz] [center_hz] [process_hz]` (requires `airspy` feature) or
/// `waterfall rtlsdr [sample_rate_hz] [center_hz] [process_hz]` (requires `rtlsdr` feature).
pub fn request_from_args() -> Option<ConnectRequest> {
    let args: Vec<String> = std::env::args().collect();
    parse_connect_request(&args)
}

pub(crate) fn parse_connect_request(args: &[String]) -> Option<ConnectRequest> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::connection::SourceKind;

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn kiwi_requires_host() {
        assert!(parse_connect_request(&argv(&["hfsdr", "kiwi"])).is_none());
    }

    #[test]
    fn kiwi_parses_host_port_and_center() {
        let req = parse_connect_request(&argv(&["hfsdr", "kiwi", "rx.test", "8074", "7030000"]))
            .expect("kiwi request");
        assert_eq!(req.kind, SourceKind::Kiwi);
        assert_eq!(req.host, "rx.test");
        assert_eq!(req.port, 8074);
        assert!((req.center_hz - 7_030_000.0).abs() < 1.0);
    }

    #[test]
    fn kiwi_defaults_port_and_center() {
        let req = parse_connect_request(&argv(&["hfsdr", "kiwi", "rx.test"])).expect("kiwi");
        assert_eq!(req.port, 8073);
        assert!((req.center_hz - 14_010_000.0).abs() < 1.0);
    }

    #[cfg(feature = "airspy")]
    #[test]
    fn airspy_parses_rates_and_process_hz() {
        let req = parse_connect_request(&argv(&["hfsdr", "airspy", "768000", "14020000", "48000"]))
            .expect("airspy");
        assert_eq!(req.kind, SourceKind::Airspy);
        assert_eq!(req.sample_rate, 768_000);
        assert!((req.center_hz - 14_020_000.0).abs() < 1.0);
        assert_eq!(req.airspy.iq_process_hz, 48_000);
    }

    #[cfg(feature = "rtlsdr")]
    #[test]
    fn rtlsdr_parses_rates_and_process_hz() {
        let req = parse_connect_request(&argv(&["hfsdr", "rtlsdr", "2048000", "7030000", "96000"]))
            .expect("rtlsdr");
        assert_eq!(req.kind, SourceKind::RtlSdr);
        assert_eq!(req.sample_rate, 2_048_000);
        assert!((req.center_hz - 7_030_000.0).abs() < 1.0);
        assert_eq!(req.rtlsdr.iq_process_hz, 96_000);
    }

    #[test]
    fn unknown_subcommand_returns_none() {
        assert!(parse_connect_request(&argv(&["hfsdr"])).is_none());
        assert!(parse_connect_request(&argv(&["hfsdr", "help"])).is_none());
    }
}
