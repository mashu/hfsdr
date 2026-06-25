//! Pure app-logic tests (no egui harness).

use std::time::Instant;

use hfsdr::skimmer::peaks::offset_hz_to_bin;
use hfsdr::{Spot, SpotKind};

use crate::app::WaterfallApp;
use crate::audio;
use crate::engine::{ConnState, EnginePoll, EngineStats, FFT_SIZE};
use crate::interaction::{CW_PASSBAND_MAX_HZ, CW_PASSBAND_NARROW_MAX_HZ};
use crate::source::{
    AirspySettings, ConnectRequest, KiwiSettings, QmxSettings, RtlSdrSettings, SourceKind,
};

fn test_app() -> WaterfallApp {
    audio::set_test_output_devices(Some(vec!["Test Output".into()]));
    WaterfallApp::new_for_test(None)
}

fn kiwi_request(host: &str) -> ConnectRequest {
    ConnectRequest {
        kind: SourceKind::Kiwi,
        host: host.into(),
        port: 8073,
        center_hz: 14_010_000.0,
        sample_rate: 0,
        kiwi: KiwiSettings::default(),
        airspy: AirspySettings::default(),
        rtlsdr: RtlSdrSettings::default(),
        qmx: QmxSettings::default(),
    }
}

fn spot(call: &str, snr: f32, kind: SpotKind) -> Spot {
    let now = Instant::now();
    Spot {
        frequency_hz: 14_010_000.0,
        callsign: Some(call.into()),
        kind,
        snr_db: snr,
        wpm: 24.0,
        first_heard: now,
        last_heard: now,
        sources: Vec::new(),
        callsign_rank: 0,
    }
}

#[test]
fn cw_band_for_center_finds_20m() {
    let band = WaterfallApp::cw_band_for_center(14_010_000.0).expect("20m band");
    assert_eq!(band.label, "20m");
}

#[test]
fn cw_band_for_center_unknown_returns_none() {
    assert!(WaterfallApp::cw_band_for_center(16_000_000.0).is_none());
}

#[test]
fn clamp_center_to_ham_bands_snaps_gap() {
    let mut app = test_app();
    app.radio.lock_ham_bands = true;
    app.radio.center_khz = 16_000.0;
    app.clamp_center_to_ham_bands();
    assert_eq!(app.radio.center_khz, 14_350.0);
}

#[test]
fn clear_rit_clears_pitch_lock() {
    let mut app = test_app();
    app.radio.rit_hz = 120.0;
    app.radio.pitch_lock = true;
    app.clear_rit();
    assert_eq!(app.radio.rit_hz, 0.0);
    assert!(!app.radio.pitch_lock);
}

#[test]
fn listen_offset_hz_sums_rit_and_preview() {
    let mut app = test_app();
    app.radio.rit_hz = 100.0;
    app.plot.tune_preview_offset_hz = Some(50.0);
    assert_eq!(app.listen_offset_hz(), 150.0);
}

#[test]
fn zero_beat_moves_center_to_peak() {
    let mut app = test_app();
    app.radio.lock_ham_bands = false;
    app.radio.center_khz = 14_010.0;
    app.plot.latest = vec![-90.0; FFT_SIZE];
    let peak_hz = 400.0f32;
    let bin = offset_hz_to_bin(peak_hz, FFT_SIZE, app.radio.sample_rate);
    app.plot.latest[bin] = -35.0;

    app.zero_beat();

    let moved_hz = app.radio.center_khz * 1000.0 - 14_010_000.0;
    assert!((moved_hz - peak_hz as f64).abs() < 100.0);
    assert_eq!(app.radio.rit_hz, 0.0);
}

#[test]
fn apply_pitch_lock_tracks_offset_peak() {
    let mut app = test_app();
    app.radio.pitch_lock = true;
    app.radio.rit_hz = 0.0;
    app.plot.latest = vec![-90.0; FFT_SIZE];
    let peak_hz = 200.0f32;
    let bin = offset_hz_to_bin(peak_hz, FFT_SIZE, app.radio.sample_rate);
    app.plot.latest[bin] = -35.0;

    for _ in 0..20 {
        app.apply_pitch_lock();
    }

    assert!(app.radio.rit_hz > 50.0);
    assert!(app.radio.rit_hz < 250.0);
}

#[test]
fn band_overview_span_uses_cw_segment() {
    let mut app = test_app();
    app.radio.center_khz = 14_010.0;
    let span = app.band_overview_span_hz();
    assert!(span >= 70_000.0);
}

#[test]
fn reconnecting_poll_updates_conn_state() {
    let mut app = test_app();
    app.inject_engine_poll(EnginePoll {
        state: ConnState::Reconnecting {
            attempt: 2,
            retry_in_s: 3.5,
        },
        stats: EngineStats::default(),
        spots: Vec::new(),
        rows: Vec::new(),
        latest: vec![-90.0; FFT_SIZE],
        last_error: None,
        audio_scope: Vec::new(),
    });
    app.pump_engine();
    assert!(matches!(
        app.engine_ui.conn_state,
        ConnState::Reconnecting { attempt: 2, .. }
    ));
    let (label, _) = app.connection_status_pill();
    assert!(label.contains("RECONNECT"));
}

#[test]
fn can_connect_request_requires_kiwi_host() {
    let req = kiwi_request("");
    assert!(!WaterfallApp::can_connect_request(&req));
    assert!(WaterfallApp::can_connect_request(&kiwi_request("rx.test")));
}

#[test]
fn remember_host_deduplicates_and_caps() {
    let mut app = test_app();
    app.connection.form.recent_hosts.clear();
    let req = kiwi_request("rx.test");
    for _ in 0..10 {
        app.remember_host(&req);
    }
    assert_eq!(app.connection.form.recent_hosts.len(), 1);
    assert_eq!(app.connection.form.recent_hosts[0].host, "rx.test");

    for i in 0..12 {
        app.remember_host(&kiwi_request(&format!("host{i}")));
    }
    assert_eq!(app.connection.form.recent_hosts.len(), 8);
    assert_eq!(app.connection.form.recent_hosts[0].host, "host11");
}

#[test]
fn annotate_new_spots_adds_history_marker() {
    let mut app = test_app();
    app.skimmer_ui.skimmer_spots = vec![spot("G0ABC", 18.0, SpotKind::CallingCq)];
    app.annotate_new_spots(14_010_000.0);
    let labels = app.history_labels();
    assert_eq!(labels.len(), 1);
    assert!(labels[0].contains("G0ABC"));
    app.annotate_new_spots(14_010_000.0);
    assert_eq!(app.history_labels().len(), 1);
}

#[test]
fn visible_spots_respect_min_snr() {
    let mut app = test_app();
    app.skimmer_ui.min_spot_snr = 15.0;
    app.skimmer_ui.skimmer_spots = vec![
        spot("G0AAA", 10.0, SpotKind::Heard),
        spot("G0AAB", 20.0, SpotKind::Heard),
    ];
    let visible = app.visible_spots();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].callsign.as_deref(), Some("G0AAB"));
}

#[test]
fn passband_max_hz_follows_filter_wide() {
    let mut app = test_app();
    app.skimmer_ui.filter_wide = false;
    assert_eq!(app.passband_max_hz(), CW_PASSBAND_NARROW_MAX_HZ);
    app.skimmer_ui.filter_wide = true;
    assert_eq!(app.passband_max_hz(), CW_PASSBAND_MAX_HZ);
}

#[test]
fn estimate_display_levels_from_spectrum() {
    let mut app = test_app();
    let mut latest = vec![-82.0; FFT_SIZE];
    latest[1024] = -40.0;
    app.plot.latest = latest;
    let (ref_db, range_db) = app.estimate_display_levels().expect("levels");
    assert!(ref_db.is_finite());
    assert!(range_db > 0.0);
}

#[test]
fn tune_to_hz_updates_center() {
    let mut app = test_app();
    app.radio.lock_ham_bands = false;
    app.tune_to_hz(14_050_000.0);
    assert!((app.radio.center_khz - 14_050.0).abs() < 1e-6);
    assert_eq!(app.plot.tune_preview_offset_hz, None);
}

#[test]
fn slow_link_shows_unstable_badge() {
    let mut app = test_app();
    let mut stats = EngineStats::default();
    stats.slow = true;
    app.inject_engine_poll(EnginePoll {
        state: ConnState::Streaming,
        stats,
        spots: Vec::new(),
        rows: Vec::new(),
        latest: vec![-90.0; FFT_SIZE],
        last_error: None,
        audio_scope: Vec::new(),
    });
    app.pump_engine();
    let (label, _) = app.connection_status_pill();
    assert_eq!(label, "UNSTABLE");
}

#[test]
fn streaming_poll_appends_waterfall_rows() {
    let mut app = test_app();
    let row = vec![-90.0; FFT_SIZE];
    app.inject_engine_poll(EnginePoll {
        state: ConnState::Streaming,
        stats: EngineStats::default(),
        spots: Vec::new(),
        rows: vec![row.clone(), row.clone()],
        latest: row,
        last_error: None,
        audio_scope: Vec::new(),
    });
    app.pump_engine();
    assert_eq!(app.plot.rows.len(), 2);
}

#[test]
fn apply_connect_form_copies_kiwi_agc() {
    let mut app = test_app();
    let mut req = kiwi_request("rx.test");
    req.kiwi.rf_agc_on = false;
    app.apply_connect_form(&req);
    assert!(!app.radio.agc_rf_on);
    req.kiwi.rf_agc_on = true;
    app.apply_connect_form(&req);
    assert!(app.radio.agc_rf_on);
}

#[test]
fn can_quick_connect_uses_recent_host() {
    let mut app = test_app();
    app.connection.form.recent_hosts.clear();
    app.connection.form.host.clear();
    app.connection.form.kind = SourceKind::Kiwi;
    assert!(!app.can_quick_connect());
    app.connection.form.recent_hosts.push(kiwi_request("rx.test"));
    assert!(app.can_quick_connect());
    assert_eq!(app.quick_connect_target_label(), "rx.test:8073");
}

#[test]
fn select_cw_band_sets_center_and_clears_rit() {
    let mut app = test_app();
    app.radio.rit_hz = 200.0;
    app.radio.pitch_lock = true;
    let band = WaterfallApp::cw_band_for_center(14_010_000.0).expect("20m");
    app.select_cw_band(band);
    assert!((app.radio.center_khz - 14_010.0).abs() < 1e-6);
    assert_eq!(app.radio.rit_hz, 0.0);
    assert!(!app.radio.pitch_lock);
}

#[test]
fn default_cw_segment_hz_matches_band_preset() {
    let mut app = test_app();
    app.radio.center_khz = 14_010.0;
    let segment = app.default_cw_segment_hz();
    assert!((segment - 70_000.0).abs() < 1.0);
}

#[test]
fn center_hz_returns_khz_times_thousand() {
    let mut app = test_app();
    app.radio.center_khz = 14_010.0;
    assert_eq!(app.center_hz(), 14_010_000.0);
}

#[test]
fn spot_labels_respect_hide_heard_and_limit() {
    let mut app = test_app();
    app.skimmer_ui.spot_hide_heard_labels = true;
    app.skimmer_ui.frame_visible_spots = vec![spot("G0ABC", 20.0, SpotKind::Heard)];
    assert!(app.spot_labels(14_010_000.0).is_empty());

    app.skimmer_ui.spot_hide_heard_labels = false;
    app.skimmer_ui.frame_visible_spots = vec![spot("G0ABC", 20.0, SpotKind::CallingCq)];
    let labels = app.spot_labels(14_010_000.0);
    assert_eq!(labels.len(), 1);
    assert!(labels[0].cq);
}

#[test]
#[cfg(feature = "rtlsdr")]
fn local_rtlsdr_connects_without_host() {
    let req = ConnectRequest {
        kind: SourceKind::RtlSdr,
        host: String::new(),
        center_hz: 14_010_000.0,
        ..ConnectRequest::default()
    };
    assert!(WaterfallApp::can_connect_request(&req));
}
