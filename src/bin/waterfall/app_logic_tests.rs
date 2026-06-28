//! Pure app-logic tests (no egui harness).

use std::time::Instant;

use hfsdr::skimmer::peaks::offset_hz_to_bin;
use hfsdr::{Spot, SpotKind};

use crate::app::WaterfallApp;
use crate::audio;
use crate::engine::{ConnState, EnginePoll, EngineStats, FFT_SIZE};
use crate::interaction::{PlotAction, RIT_MAX_HZ, RIT_MIN_HZ, CW_PASSBAND_MAX_HZ, CW_PASSBAND_NARROW_MAX_HZ};
use crate::source::{
    AirspySettings, ConnectRequest, KiwiSettings, QmxSettings, RtlSdrSettings, SourceKind,
};
use crate::iq_panel::IqPanelCmd;
use crate::settings::AppSettings;

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
    app.radio.rit_on = true;
    app.radio.pitch_lock = true;
    app.clear_rit();
    assert_eq!(app.radio.rit_hz, 0.0);
    assert!(!app.radio.rit_on);
    assert!(!app.radio.pitch_lock);
}

#[test]
fn listen_offset_hz_sums_rit_and_preview() {
    let mut app = test_app();
    app.radio.rit_hz = 100.0;
    app.radio.rit_on = true;
    app.plot.tune_preview_offset_hz = Some(50.0);
    assert_eq!(app.rit_offset_hz(), 100.0);
    assert_eq!(app.tune_preview_hz(), 50.0);
    assert_eq!(app.listen_offset_hz(), 150.0);
}

#[test]
fn rit_offset_hz_zero_when_rit_off() {
    let mut app = test_app();
    app.radio.rit_hz = 200.0;
    app.radio.rit_on = false;
    assert_eq!(app.rit_offset_hz(), 0.0);
    assert_eq!(app.listen_offset_hz(), 0.0);
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
    assert!(!app.radio.rit_on);
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
    assert!(app.radio.rit_on);
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
    app.radio.passband_wide = false;
    assert_eq!(app.passband_max_hz(), CW_PASSBAND_NARROW_MAX_HZ);
    app.radio.passband_wide = true;
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
fn sideband_auto_follows_band_plan() {
    use hfsdr::CwSideband;

    let mut app = test_app();
    app.radio.sideband_auto = true;
    app.radio.lock_ham_bands = false;
    app.tune_to_hz(14_010_000.0);
    assert_eq!(app.radio.cw.sideband, CwSideband::Lower);
    app.tune_to_hz(7_010_000.0);
    assert_eq!(app.radio.cw.sideband, CwSideband::Upper);
    app.radio.sideband_auto = false;
    app.radio.cw.sideband = CwSideband::Lower;
    app.tune_to_hz(14_010_000.0);
    assert_eq!(app.radio.cw.sideband, CwSideband::Lower);
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
    app.radio.rit_on = true;
    app.radio.pitch_lock = true;
    let band = WaterfallApp::cw_band_for_center(14_010_000.0).expect("20m");
    app.select_cw_band(band);
    assert!((app.radio.center_khz - 14_010.0).abs() < 1e-6);
    assert_eq!(app.radio.rit_hz, 0.0);
    assert!(!app.radio.rit_on);
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

#[test]
fn invalidate_waterfall_history_clears_rows() {
    let mut app = test_app();
    app.plot.rows.push_back(vec![-90.0; FFT_SIZE]);
    app.invalidate_waterfall_history();
    assert!(app.plot.rows.is_empty());
    assert!(app.plot.waterfall.textures_dirty);
}

#[test]
fn waterfall_trace_row_index_follows_displayed_row_not_latest() {
    let mut app = test_app();
    app.plot.rows.push_front(vec![-50.0; FFT_SIZE]);
    app.plot.rows.push_front(vec![-70.0; FFT_SIZE]);
    app.plot.waterfall.pending_viewport_row_appends = 1;
    assert_eq!(app.waterfall_trace_row_index(), 1);
    app.plot.waterfall.pending_viewport_row_appends = 0;
    assert_eq!(app.waterfall_trace_row_index(), 0);
}

#[test]
fn pump_engine_ingests_skimmer_spots() {
    let mut app = test_app();
    let spot = spot("G0XYZ", 18.0, SpotKind::Heard);
    app.inject_engine_poll(EnginePoll {
        state: ConnState::Streaming,
        stats: EngineStats::default(),
        spots: vec![spot],
        rows: Vec::new(),
        latest: vec![-90.0; FFT_SIZE],
        last_error: None,
        audio_scope: Vec::new(),
    });
    app.pump_engine();
    assert_eq!(app.skimmer_ui.skimmer_spots.len(), 1);
    assert_eq!(
        app.skimmer_ui.skimmer_spots[0].callsign.as_deref(),
        Some("G0XYZ")
    );
}

#[test]
fn clear_spots_wipes_local_state() {
    let mut app = test_app();
    app.skimmer_ui.skimmer_spots = vec![spot("G0ABC", 20.0, SpotKind::CallingCq)];
    app.annotate_new_spots(14_010_000.0);
    assert!(!app.history_labels().is_empty());
    app.clear_spots();
    assert!(app.skimmer_ui.skimmer_spots.is_empty());
    assert!(app.skimmer_ui.frame_visible_spots.is_empty());
}

#[test]
fn plot_full_span_uses_spectrum_rate_from_stats() {
    let mut app = test_app();
    app.engine_ui.stats.spectrum_rate = 48_000.0;
    app.engine_ui.stats.iq_passband_hz = 96_000.0;
    assert_eq!(app.plot_full_span_hz(), 48_000.0);
}

#[test]
fn fft_size_change_on_poll_resets_row_buffer() {
    let mut app = test_app();
    app.plot.rows.push_back(vec![-90.0; FFT_SIZE]);
    app.inject_engine_poll(EnginePoll {
        state: ConnState::Streaming,
        stats: EngineStats::default(),
        spots: Vec::new(),
        rows: Vec::new(),
        latest: vec![-90.0; 1024],
        last_error: None,
        audio_scope: Vec::new(),
    });
    app.pump_engine();
    assert_eq!(app.plot.latest.len(), 1024);
    assert!(app.plot.rows.is_empty());
}

#[test]
fn skimmer_runtime_disabled_when_span_too_wide() {
    let mut app = test_app();
    app.skimmer_ui.skimmer_enabled = true;
    app.radio.is_kiwi = false;
    app.engine_ui.stats.spectrum_rate = 200_000.0;
    app.engine_ui.stats.iq_passband_hz = 200_000.0;
    assert!(!app.skimmer_spectrum_ok());
    assert!(!app.skimmer_runtime_enabled());
}

#[test]
fn skimmer_runtime_enabled_on_kiwi_despite_wide_span() {
    let mut app = test_app();
    app.skimmer_ui.skimmer_enabled = true;
    app.radio.is_kiwi = true;
    app.engine_ui.stats.spectrum_rate = 200_000.0;
    assert!(app.skimmer_spectrum_ok());
    assert!(app.skimmer_runtime_enabled());
}

#[test]
fn effective_skimmer_caps_channels_on_wideband() {
    let mut app = test_app();
    app.skimmer_ui.skimmer.max_channels = 32;
    app.radio.is_kiwi = false;
    app.engine_ui.stats.iq_passband_hz = 384_000.0;
    app.engine_ui.stats.sample_rate = 384_000.0;
    let cfg = app.effective_skimmer();
    assert!(cfg.max_channels <= 8);
}

#[test]
fn effective_skimmer_uses_connection_alias_when_streaming() {
    let mut app = test_app();
    app.connection.form.host = "rx.test".into();
    app.engine_ui.conn_state = ConnState::Streaming;
    let cfg = app.effective_skimmer();
    assert_eq!(cfg.source_label, "rx.test:8073");
}

#[test]
fn effective_target_fps_caps_wideband() {
    let mut app = test_app();
    app.display.target_fps = 60;
    app.radio.is_kiwi = false;
    app.engine_ui.stats.iq_passband_hz = 384_000.0;
    assert_eq!(app.effective_target_fps(), 15);
}

#[test]
fn connection_session_live_during_connect_and_stream() {
    let mut app = test_app();
    assert!(!app.connection_session_live());
    app.engine_ui.conn_state = ConnState::Connecting {
        label: "rx".into(),
    };
    assert!(app.connection_session_live());
    app.engine_ui.conn_state = ConnState::Streaming;
    assert!(app.connection_session_live());
    app.engine_ui.conn_state = ConnState::Reconnecting {
        attempt: 1,
        retry_in_s: 2.0,
    };
    assert!(app.connection_session_live());
}

#[test]
fn connection_alias_defaults_for_empty_kiwi_host() {
    let mut app = test_app();
    app.connection.form.host.clear();
    assert_eq!(app.connection_alias(), "KiwiSDR");
    app.connection.form.host = "rx.test".into();
    app.connection.form.port = 8073;
    assert_eq!(app.connection_alias(), "rx.test:8073");
}

#[test]
fn apply_settings_restores_bfo_and_passband() {
    let mut app = test_app();
    let mut saved = AppSettings::default();
    saved.bfo_hz = 550.0;
    saved.passband_hz = 300.0;
    app.apply_settings(&saved);
    assert!((app.radio.cw.bfo_hz - 550.0).abs() < 1e-6);
    assert!((app.radio.cw.passband_hz - 300.0).abs() < 1e-6);
}

#[test]
fn toggle_pipeline_stage_flips_noise_blanker() {
    let mut app = test_app();
    let before = app.radio.cw.noise_blanker.enabled;
    app.toggle_pipeline_stage(crate::pipeline_flow::PipelineStage::NoiseBlanker);
    assert_ne!(app.radio.cw.noise_blanker.enabled, before);
}

#[test]
fn toggle_notch_bypass_stashes_and_restores() {
    let mut app = test_app();
    app.radio.cw.notches[0].enabled = true;
    app.toggle_notch_bypass();
    assert!(!app.radio.cw.notches[0].enabled);
    assert!(app.chrome.notch_bypass_stash.is_some());
    app.toggle_notch_bypass();
    assert!(app.radio.cw.notches[0].enabled);
}

#[test]
fn arm_manual_notch_sets_offset() {
    let mut app = test_app();
    app.arm_manual_notch(0, None);
    assert!(app.radio.cw.notches[0].enabled);
    assert!(app.radio.cw.notches[0].width_hz >= 50.0);
}

#[test]
fn enabled_notches_lists_active_slots() {
    let mut app = test_app();
    app.arm_manual_notch(1, Some(hfsdr::ChannelOffsetHz::new(120.0)));
    let overlay = app.filter_overlay_cached().clone();
    let markers = app.enabled_notches(&overlay);
    assert_eq!(markers.len(), 1);
    assert_eq!(markers[0].slot, 1);
}

#[test]
fn pipeline_ingress_decim_kiwi_default() {
    let mut app = test_app();
    app.connection.form.kind = SourceKind::Kiwi;
    app.connection.form.sample_rate = 12_000;
    assert!(app.pipeline_ingress_decim() >= 1);
}

#[test]
fn apply_connect_form_syncs_kiwi_agc() {
    let mut app = test_app();
    let mut req = kiwi_request("rx.test");
    req.kiwi.rf_agc_on = false;
    app.apply_connect_form(&req);
    assert!(!app.radio.agc_rf_on);
}

#[test]
fn sync_kiwi_rf_sends_when_streaming() {
    let mut app = test_app();
    app.connection.form.kind = SourceKind::Kiwi;
    app.engine_ui.conn_state = ConnState::Streaming;
    app.radio.agc_rf_on = false;
    app.radio.last_agc_rf_on = true;
    app.sync_kiwi_rf_now();
    assert_eq!(app.radio.last_agc_rf_on, false);
}

#[test]
fn apply_radio_settings_tunes_on_center_change() {
    let mut app = test_app();
    app.radio.center_khz = 14_020.0;
    app.radio.last_center_khz = 14_010.0;
    app.apply_radio_settings();
    assert_eq!(app.radio.last_center_khz, 14_020.0);
}

#[test]
fn apply_plot_actions_tune_delta_hz() {
    let mut app = test_app();
    app.radio.center_khz = 14_010.0;
    app.apply_plot_actions(vec![PlotAction::TuneDeltaHz(500.0)]);
    assert!((app.radio.center_khz - 14_010.5).abs() < 1e-6);
}

#[test]
fn apply_plot_actions_center_on_offset_clears_rit() {
    let mut app = test_app();
    app.radio.lock_ham_bands = false;
    app.radio.center_khz = 14_010.0;
    app.radio.rit_hz = 80.0;
    app.apply_plot_actions(vec![PlotAction::CenterOnOffsetHz(200.0)]);
    assert_eq!(app.radio.rit_hz, 0.0);
    assert!((app.radio.center_khz - 14_010.2).abs() < 1e-3);
}

#[test]
fn apply_plot_actions_iq_playback_pans_view() {
    let mut app = test_app();
    app.engine_ui.stats.iq_playback = true;
    app.engine_ui.stats.sample_rate = 12_000.0;
    app.engine_ui.stats.iq_passband_hz = 12_000.0;
    app.plot.plot_view.zoom = 0.25;
    let before = app.plot.plot_view.pan_offset_hz;
    app.apply_plot_actions(vec![PlotAction::PanViewDeltaHz(100.0)]);
    assert!((app.plot.plot_view.pan_offset_hz - before - 100.0).abs() < 1e-3);
}

#[test]
fn apply_plot_actions_passband_clamp() {
    let mut app = test_app();
    app.apply_plot_actions(vec![PlotAction::SetPassbandHz(10_000.0)]);
    assert!(app.radio.cw.passband_hz <= CW_PASSBAND_MAX_HZ);
}

#[test]
fn rit_clamps_to_limits() {
    let mut app = test_app();
    app.radio.rit_hz = RIT_MAX_HZ + 500.0;
    app.radio.rit_hz = app.radio.rit_hz.clamp(RIT_MIN_HZ, RIT_MAX_HZ);
    assert_eq!(app.radio.rit_hz, RIT_MAX_HZ);
}

#[test]
fn apply_plot_actions_zoom_and_pan() {
    let mut app = test_app();
    app.engine_ui.stats.sample_rate = 12_000.0;
    app.engine_ui.stats.iq_passband_hz = 12_000.0;
    app.apply_plot_actions(vec![
        PlotAction::ZoomView(0.5),
        PlotAction::PanViewDeltaHz(50.0),
        PlotAction::SetViewPanHz(25.0),
    ]);
    assert!(app.plot.plot_view.zoom < 1.0);
}

#[test]
fn apply_plot_actions_notch_edits() {
    let mut app = test_app();
    app.arm_manual_notch(0, Some(hfsdr::ChannelOffsetHz::new(100.0)));
    app.apply_plot_actions(vec![
        PlotAction::SetNotchOffset {
            slot: 0,
            offset_hz: hfsdr::ChannelOffsetHz::new(200.0),
        },
        PlotAction::SetNotchWidth {
            slot: 0,
            width_hz: 120.0,
        },
    ]);
    assert_eq!(app.radio.cw.notches[0].offset_hz.hz(), 200.0);
    assert_eq!(app.radio.cw.notches[0].width_hz, 120.0);
}

#[test]
fn estimate_display_levels_from_rows() {
    let mut app = test_app();
    app.engine_ui.stats.sample_rate = 12_000.0;
    app.engine_ui.stats.spectrum_rate = 12_000.0;
    app.plot.latest = vec![-80.0; FFT_SIZE];
    for _ in 0..12 {
        app.plot.rows.push_back(vec![-75.0; FFT_SIZE]);
    }
    app.display.display_auto_track = true;
    app.update_display_levels();
    assert!(app.display.display_levels_initialized);
    assert!(app.display.ref_db.is_finite());
}

#[test]
fn passband_max_respects_passband_wide() {
    let mut app = test_app();
    app.radio.passband_wide = false;
    assert_eq!(app.passband_max_hz(), CW_PASSBAND_NARROW_MAX_HZ);
    app.radio.passband_wide = true;
    assert_eq!(app.passband_max_hz(), CW_PASSBAND_MAX_HZ);
}

#[test]
fn spectrum_and_waterfall_views_finite() {
    let mut app = test_app();
    app.engine_ui.stats.sample_rate = 96_000.0;
    app.engine_ui.stats.iq_passband_hz = 96_000.0;
    app.engine_ui.stats.spectrum_rate = 96_000.0;
    let view = app.spectrum_view();
    assert!(view.view_span_hz.is_finite() && view.view_span_hz > 0.0);
    let storage = app.waterfall_storage_view();
    assert!(storage.data_span_hz.is_finite());
}

#[test]
fn connect_now_builds_request() {
    let mut app = test_app();
    app.connection.form.host = "rx.test".into();
    app.connection.form.recent_hosts.clear();
    app.connect_now();
    assert_eq!(app.radio.last_center_khz, app.radio.center_khz);
}

#[test]
fn cancel_connection_does_not_panic() {
    let mut app = test_app();
    app.cancel_connection();
}

#[test]
fn quick_connect_uses_recent_host() {
    let mut app = test_app();
    app.connection.form.recent_hosts.clear();
    app.remember_host(&kiwi_request("recent.test"));
    app.quick_connect_last();
    assert_eq!(app.connection.form.host, "recent.test");
}

#[test]
fn toggle_all_pipeline_stages() {
    let mut app = test_app();
    use crate::pipeline_flow::PipelineStage;
    for stage in [
        PipelineStage::NoiseBlanker,
        PipelineStage::ManualNotches,
        PipelineStage::ListenNco,
        PipelineStage::DecimatorFir,
        PipelineStage::ChannelFir,
        PipelineStage::Bfo,
        PipelineStage::Agc,
        PipelineStage::Apf,
        PipelineStage::AutoNotch,
        PipelineStage::NoiseReduction,
        PipelineStage::Skimmer,
        PipelineStage::AudioOutput,
    ] {
        app.toggle_pipeline_stage(stage);
    }
}

#[test]
fn pump_engine_ingests_injected_poll() {
    let mut app = test_app();
    app.inject_engine_poll(EnginePoll {
        state: ConnState::Streaming,
        stats: {
            let mut s = EngineStats::default();
            s.sample_rate = 12_000.0;
            s.iq_passband_hz = 12_000.0;
            s.spectrum_rate = 12_000.0;
            s.spectrum_fft = FFT_SIZE;
            s.is_kiwi = true;
            s.snr_db = 10.0;
            s
        },
        spots: Vec::new(),
        rows: vec![vec![-90.0; FFT_SIZE]],
        latest: vec![-90.0; FFT_SIZE],
        last_error: None,
        audio_scope: vec![0.0; 64],
    });
    app.pump_engine();
    assert_eq!(app.radio.sample_rate, 12_000.0);
    assert!(app.plot.latest_frame_tick);
}

#[test]
fn poll_kiwi_directory_applies_fetch_result() {
    let mut app = test_app();
    let (tx, rx) = std::sync::mpsc::channel();
    let geo = crate::kiwi_directory::GeoLocation {
        country: "Test".into(),
        country_code: "TS".into(),
        lat: 51.0,
        lon: 0.0,
    };
    tx.send(Ok((Some(geo.clone()), Vec::new()))).unwrap();
    app.connection.kiwi.fetch_rx = Some(rx);
    app.poll_kiwi_directory();
    assert!(app.connection.kiwi.fetch_rx.is_none());
    assert_eq!(app.connection.kiwi.geo.as_ref().map(|g| g.country.as_str()), Some("Test"));
}

#[test]
fn connection_unstable_during_reconnect() {
    let mut app = test_app();
    app.engine_ui.conn_state = ConnState::Reconnecting {
        attempt: 1,
        retry_in_s: 2.0,
    };
    assert!(app.connection_unstable());
}

#[test]
fn skimmer_runtime_disabled_when_spectrum_too_wide() {
    let mut app = test_app();
    app.skimmer_ui.skimmer_enabled = true;
    app.radio.is_kiwi = false;
    app.engine_ui.stats.iq_passband_hz = 384_000.0;
    app.engine_ui.stats.sample_rate = 384_000.0;
    app.engine_ui.stats.spectrum_rate = 384_000.0;
    assert!(!app.skimmer_runtime_enabled());
}

#[test]
fn process_iq_cmds_stop_playback() {
    let mut app = test_app();
    app.process_iq_cmds(vec![IqPanelCmd::StopPlayback, IqPanelCmd::StopRecord]);
}

#[test]
fn start_kiwi_directory_fetch_sets_receiver() {
    let mut app = test_app();
    app.start_kiwi_directory_fetch(false);
    assert!(app.connection.kiwi.fetch_rx.is_some());
}
