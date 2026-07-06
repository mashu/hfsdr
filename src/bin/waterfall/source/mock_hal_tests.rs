//! Integration tests exercising `connect()` with mock HAL backends.

use std::sync::atomic::AtomicBool;

use hfsdr::mock_hal::MockGuard;

use super::connection::{connect, ConnectRequest, SourceKind};
use super::settings::{AirspySettings, KiwiSettings, QmxSettings, RtlSdrSettings};

#[test]
fn mock_connect_kiwi() {
    let _guard = MockGuard::new();
    let cancel = AtomicBool::new(false);
    let req = ConnectRequest {
        kind: SourceKind::Kiwi,
        host: "mock.kiwi.test".into(),
        port: 8073,
        center_hz: 14_010_000.0,
        kiwi: KiwiSettings::default(),
        ..ConnectRequest::default()
    };
    let conn = connect(&req, &cancel).expect("mock kiwi connect");
    assert!(conn.is_kiwi);
    assert!(conn.sample_rate > 0.0);
}

#[cfg(feature = "airspy")]
#[test]
fn mock_connect_airspy() {
    let _guard = MockGuard::new();
    let cancel = AtomicBool::new(false);
    let req = ConnectRequest {
        kind: SourceKind::Airspy,
        center_hz: 14_010_000.0,
        sample_rate: 384_000,
        airspy: AirspySettings {
            hf_agc: true,
            hf_att: 2,
            hf_lna: true,
            ..AirspySettings::default()
        },
        ..ConnectRequest::default()
    };
    let conn = connect(&req, &cancel).expect("mock airspy connect");
    assert!(!conn.is_kiwi);
    assert_eq!(conn.device_sample_rate, 384_000.0);
}

#[cfg(feature = "rtlsdr")]
#[test]
fn mock_connect_rtlsdr() {
    let _guard = MockGuard::new();
    let cancel = AtomicBool::new(false);
    let req = ConnectRequest {
        kind: SourceKind::RtlSdr,
        center_hz: 7_100_000.0,
        rtlsdr: RtlSdrSettings {
            device_index: 0,
            manual_gain: true,
            tuner_gain_db10: 196,
            ..RtlSdrSettings::default()
        },
        ..ConnectRequest::default()
    };
    let conn = connect(&req, &cancel).expect("mock rtlsdr connect");
    assert!(!conn.is_kiwi);
    assert!(conn.sample_rate > 0.0);
}

#[cfg(feature = "qmx")]
#[test]
fn mock_connect_qmx() {
    let _guard = MockGuard::new();
    let cancel = AtomicBool::new(false);
    let req = ConnectRequest {
        kind: SourceKind::Qmx,
        center_hz: 14_200_000.0,
        qmx: QmxSettings {
            serial_port: "mock".into(),
            audio_device: "mock".into(),
            rf_gain_db: 10,
            ..QmxSettings::default()
        },
        ..ConnectRequest::default()
    };
    let conn = connect(&req, &cancel).expect("mock qmx connect");
    assert!(!conn.is_kiwi);
    assert_eq!(conn.device_sample_rate, 48_000.0);
}

#[test]
fn mock_connect_respects_cancel_flag() {
    let _guard = MockGuard::new();
    let cancel = AtomicBool::new(true);
    let req = ConnectRequest {
        kind: SourceKind::Kiwi,
        host: "mock.kiwi.test".into(),
        ..ConnectRequest::default()
    };
    let err = connect(&req, &cancel).err().expect("cancelled");
    assert!(err.contains("cancelled"));
}

#[cfg(feature = "soapy")]
#[test]
fn mock_connect_soapy_pluto_network() {
    use super::settings::SoapySettings;
    use hfsdr::mock_hal::MOCK_PLUTO_NET_ARGS;

    let _guard = MockGuard::new();
    let cancel = AtomicBool::new(false);
    let req = ConnectRequest {
        kind: SourceKind::Soapy,
        center_hz: 7_100_000.0,
        sample_rate: 2_083_333,
        soapy: SoapySettings {
            device_args: MOCK_PLUTO_NET_ARGS.into(),
            driver: "plutosdr".into(),
            agc: true,
            ..SoapySettings::default()
        },
        ..ConnectRequest::default()
    };
    let conn = connect(&req, &cancel).expect("mock soapy pluto network connect");
    assert!(!conn.is_kiwi);
    assert_eq!(conn.center_hz, 7_100_000.0);
    assert!(conn.sample_rate > 0.0);
}

#[cfg(feature = "soapy")]
#[test]
fn mock_connect_soapy_pluto_streams_iq() {
    use super::settings::SoapySettings;
    use hfsdr::mock_hal::MOCK_PLUTO_USB_ARGS;
    use rtrb::RingBuffer;

    let _guard = MockGuard::new();
    let cancel = AtomicBool::new(false);
    let req = ConnectRequest {
        kind: SourceKind::Soapy,
        center_hz: 14_010_000.0,
        sample_rate: 2_083_333,
        soapy: SoapySettings {
            device_args: MOCK_PLUTO_USB_ARGS.into(),
            driver: "plutosdr".into(),
            agc: false,
            gain_db: 40.0,
            antenna: "B".into(),
            ..SoapySettings::default()
        },
        ..ConnectRequest::default()
    };
    let mut conn = connect(&req, &cancel).expect("mock pluto connect");
    let (mut prod, mut cons) = RingBuffer::<hfsdr::Complex32>::new(4096);
    std::thread::sleep(std::time::Duration::from_millis(40));
    let mut got = 0usize;
    while let Ok(sample) = conn.iq.pop() {
        let _ = prod.push(sample);
        got += 1;
        if got >= 8 {
            break;
        }
    }
    assert!(got >= 1, "expected IQ samples from mock Pluto stream");
    assert!(cons.pop().is_ok());
    let _ = conn.device.stop();
}
