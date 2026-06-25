//! Headless egui smoke tests for [`WaterfallApp`].

use eframe::egui::Vec2;
use egui_kittest::{Harness, kittest::Queryable as _};

use crate::app::WaterfallApp;
use crate::audio;
use crate::engine::{ConnState, EnginePoll, EngineStats, FFT_SIZE};
use crate::meters::rf_level_dbm;
use crate::theme;

const TEST_AUDIO_DEVICES: [&str; 1] = ["Test Output"];

fn install_test_audio_devices() {
    audio::set_test_output_devices(Some(
        TEST_AUDIO_DEVICES.iter().map(|s| (*s).to_string()).collect(),
    ));
}

fn test_harness() -> Harness<'static, WaterfallApp> {
    install_test_audio_devices();
    Harness::builder()
        .with_size(Vec2::new(1580.0, 960.0))
        .with_max_steps(64)
        .with_wait_for_pending_images(false)
        .build_eframe(|cc| {
            theme::apply(&cc.egui_ctx);
            WaterfallApp::new_for_test(None)
        })
}

fn streaming_stats() -> EngineStats {
    let mut stats = EngineStats::default();
    stats.sample_rate = 96_000.0;
    stats.iq_passband_hz = 96_000.0;
    stats.spectrum_fft = FFT_SIZE;
    stats.effective_sps = 96_000.0;
    stats.snr_db = 18.0;
    stats.iq_rf_level = 0.05;
    stats.agc_gain = 1.2;
    stats.agc_envelope = 0.08;
    stats.audio_peak = 0.3;
    stats.audio_rms = 0.15;
    stats
}

fn synthetic_streaming_poll(frame: usize) -> EnginePoll {
    let mut latest = vec![-90.0; FFT_SIZE];
    for (i, v) in latest.iter_mut().enumerate() {
        *v = -90.0 + ((i + frame * 17) % 50) as f32 * 0.1;
    }
    EnginePoll {
        state: ConnState::Streaming,
        stats: streaming_stats(),
        spots: Vec::new(),
        rows: vec![latest.clone()],
        latest,
        last_error: None,
        audio_scope: vec![0.0; 128],
    }
}

fn poll_with_latest(latest: Vec<f32>, rows: Vec<Vec<f32>>) -> EnginePoll {
    EnginePoll {
        state: ConnState::Streaming,
        stats: streaming_stats(),
        spots: Vec::new(),
        rows,
        latest,
        last_error: None,
        audio_scope: vec![0.0; 128],
    }
}

fn poll_with_stats(stats: EngineStats) -> EnginePoll {
    let latest = vec![-90.0; FFT_SIZE];
    EnginePoll {
        state: ConnState::Streaming,
        stats,
        spots: Vec::new(),
        rows: vec![latest.clone()],
        latest,
        last_error: None,
        audio_scope: vec![0.0; 128],
    }
}

fn assert_ui_values_finite(app: &WaterfallApp) {
    let stats = &app.engine_ui.stats;
    assert!(stats.sample_rate.is_finite() && stats.sample_rate > 0.0);
    assert!(stats.snr_db.is_finite());
    assert!(stats.iq_rf_level.is_finite());
    assert!(stats.agc_gain.is_finite() && stats.agc_gain > 0.0);
    assert!(stats.agc_envelope.is_finite());
    assert!(stats.audio_peak.is_finite());
    assert!(stats.audio_rms.is_finite());
    assert!(app.plot.latest.iter().all(|v| v.is_finite()));
    assert_eq!(app.plot.latest.len(), FFT_SIZE);
    assert!(app.display.ref_db.is_finite());
    assert!(app.display.range_db.is_finite());
    assert!(app.display.range_db > 0.0);

    let rf_dbm = app.rf_meter_dbm();
    assert!(rf_dbm.is_finite());
    assert!(rf_level_dbm(stats.rssi_dbm, stats.iq_rf_level).is_finite());
}

fn inject_and_step(harness: &mut Harness<'_, WaterfallApp>, poll: EnginePoll, steps: usize) {
    harness.state().inject_engine_poll(poll);
    harness.run_steps(steps);
}

fn run_streaming_frames(harness: &mut Harness<'_, WaterfallApp>, frames: usize) {
    for frame in 0..frames {
        inject_and_step(harness, synthetic_streaming_poll(frame), 1);
    }
}

#[test]
fn startup_smoke() {
    let mut harness = test_harness();
    harness.run_steps(3);
    assert_eq!(
        harness.state().audio.audio_devices,
        vec!["Test Output".to_string()]
    );
}

#[test]
fn disconnected_shows_offline() {
    let mut harness = test_harness();
    harness.run_steps(1);
    harness.get_by_label("OFFLINE");
}

#[test]
fn click_offline_opens_connection_panel() {
    let mut harness = test_harness();
    harness.run_steps(1);
    harness.get_by_label("OFFLINE").click();
    harness.run_steps(2);
    harness.get_by_label("Connection");
}

#[test]
fn synthetic_streaming_poll_updates_ui() {
    let mut harness = test_harness();
    harness.run_steps(1);

    inject_and_step(&mut harness, synthetic_streaming_poll(0), 2);

    let app = harness.state();
    assert!(matches!(app.engine_ui.conn_state, ConnState::Streaming));
    assert_eq!(app.radio.sample_rate, 96_000.0);
    assert_ui_values_finite(app);
    harness.get_by_label("STREAMING");
}

#[test]
fn streaming_multi_frame_polls_stay_finite() {
    let mut harness = test_harness();
    harness.run_steps(1);
    run_streaming_frames(&mut harness, 8);
    assert_ui_values_finite(harness.state());
    assert!(matches!(
        harness.state().engine_ui.conn_state,
        ConnState::Streaming
    ));
}

#[test]
fn nan_spectrum_bins_sanitize_to_finite() {
    let mut harness = test_harness();
    harness.run_steps(1);

    let mut latest = vec![-90.0; FFT_SIZE];
    latest[128] = f32::NAN;
    latest[512] = f32::INFINITY;
    latest[1024] = f32::NEG_INFINITY;
    inject_and_step(
        &mut harness,
        poll_with_latest(latest, vec![vec![-90.0; FFT_SIZE]]),
        2,
    );

    assert_ui_values_finite(harness.state());
}

#[test]
fn empty_spectrum_preserves_fft_width() {
    let mut harness = test_harness();
    harness.run_steps(1);

    inject_and_step(
        &mut harness,
        poll_with_latest(Vec::new(), Vec::new()),
        2,
    );

    assert_ui_values_finite(harness.state());
}

#[test]
fn extreme_dbm_stats_clamp_needle() {
    let mut harness = test_harness();
    harness.run_steps(1);

    let mut stats = streaming_stats();
    stats.iq_rf_level = 1_000.0;
    stats.rssi_dbm = Some(50.0);
    stats.snr_db = 999.0;
    stats.audio_peak = 5.0;
    stats.agc_gain = 100.0;
    inject_and_step(&mut harness, poll_with_stats(stats), 2);

    let app = harness.state();
    assert_ui_values_finite(app);
    let rf_dbm = app.rf_meter_dbm();
    assert!(rf_dbm <= -33.0);
    assert!(rf_dbm >= -127.0);
}

#[test]
fn nan_engine_stats_sanitize() {
    let mut harness = test_harness();
    harness.run_steps(1);

    let mut stats = streaming_stats();
    stats.sample_rate = f32::NAN;
    stats.snr_db = f32::NAN;
    stats.iq_rf_level = f32::NAN;
    stats.agc_gain = f32::NAN;
    stats.agc_envelope = f32::INFINITY;
    stats.audio_peak = f32::NAN;
    stats.audio_rms = f32::NEG_INFINITY;
    inject_and_step(&mut harness, poll_with_stats(stats), 2);

    assert_ui_values_finite(harness.state());
}

#[test]
fn flat_noise_floor_spectrum() {
    let mut harness = test_harness();
    harness.run_steps(1);

    let latest = vec![-120.0; FFT_SIZE];
    inject_and_step(
        &mut harness,
        poll_with_latest(latest.clone(), vec![latest]),
        2,
    );

    assert_ui_values_finite(harness.state());
}

#[test]
fn slow_link_shows_unstable_badge() {
    let mut harness = test_harness();
    harness.run_steps(1);

    let mut stats = streaming_stats();
    stats.slow = true;
    inject_and_step(&mut harness, poll_with_stats(stats), 2);

    harness.get_by_label("UNSTABLE");
    assert_ui_values_finite(harness.state());
}

#[test]
fn reconnecting_shows_retry_label() {
    let mut harness = test_harness();
    harness.run_steps(1);

    harness.state().inject_engine_poll(EnginePoll {
        state: ConnState::Reconnecting {
            attempt: 1,
            retry_in_s: 2.0,
        },
        stats: streaming_stats(),
        spots: Vec::new(),
        rows: Vec::new(),
        latest: vec![-90.0; FFT_SIZE],
        last_error: None,
        audio_scope: Vec::new(),
    });
    harness.run_steps(2);
    harness.get_by_label("RECONNECT #1 (2s)");
}

#[test]
fn connecting_shows_connecting_badge() {
    let mut harness = test_harness();
    harness.run_steps(1);

    harness.state().inject_engine_poll(EnginePoll {
        state: ConnState::Connecting {
            label: "rx.test:8073".into(),
        },
        stats: streaming_stats(),
        spots: Vec::new(),
        rows: Vec::new(),
        latest: vec![-90.0; FFT_SIZE],
        last_error: None,
        audio_scope: Vec::new(),
    });
    harness.run_steps(2);
    harness.get_by_label("CONNECTING");
}
