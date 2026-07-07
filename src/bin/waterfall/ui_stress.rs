//! Stress and perturbation tests — rapid state changes, corrupt engine polls, layout extremes.

use std::time::Instant;

use eframe::egui::{Key, Vec2};
use egui_kittest::{Harness, kittest::Queryable as _};

use crate::app::WaterfallApp;
use crate::audio;
use crate::engine::{ConnState, EnginePoll, FFT_SIZE};
use crate::pipeline_flow::PipelineStage;
use crate::source::SourceKind;
use crate::theme;
use crate::ui_smoke::{inject_and_step, streaming_stats, synthetic_streaming_poll};
use hfsdr::{Spot, SpotKind};

const TEST_AUDIO_DEVICES: [&str; 1] = ["Test Output"];

fn stress_harness(size: Vec2, max_steps: u64) -> Harness<'static, WaterfallApp> {
    audio::set_test_output_devices(Some(
        TEST_AUDIO_DEVICES.iter().map(|s| (*s).to_string()).collect(),
    ));
    Harness::builder()
        .with_size(size)
        .with_max_steps(max_steps)
        .with_wait_for_pending_images(false)
        .build_eframe(|cc| {
            theme::apply(&cc.egui_ctx);
            WaterfallApp::new_for_test(None)
        })
}

fn assert_ui_finite(app: &WaterfallApp) {
    let stats = &app.engine_ui.stats;
    assert!(stats.sample_rate.is_finite());
    assert!(stats.snr_db.is_finite());
    assert!(stats.iq_rf_level.is_finite());
    assert!(stats.agc_gain.is_finite() && stats.agc_gain > 0.0);
    assert!(stats.audio_peak.is_finite());
    assert!(stats.audio_rms.is_finite());
    assert!(app.display.ref_db.is_finite());
    assert!(app.display.range_db.is_finite() && app.display.range_db > 0.0);
    assert!(app.plot.latest.iter().all(|v| v.is_finite()));
    assert!(app.rf_meter_dbm().is_finite());
}

fn corrupt_poll(frame: usize) -> EnginePoll {
    let mut latest = vec![-90.0; FFT_SIZE];
    for (i, v) in latest.iter_mut().enumerate() {
        *v = match (frame + i) % 17 {
            0 => f32::NAN,
            1 => f32::INFINITY,
            2 => f32::NEG_INFINITY,
            3 => -200.0,
            4 => 50.0,
            _ => -90.0 + ((i + frame * 13) % 80) as f32 * 0.25,
        };
    }
    let mut stats = streaming_stats();
    if frame % 5 == 0 {
        stats.snr_db = f32::NAN;
        stats.audio_peak = f32::INFINITY;
    }
    if frame % 7 == 0 {
        stats.sample_rate = 0.0;
    }
    if frame % 11 == 0 {
        stats.slow = true;
    }
    EnginePoll {
        state: if frame % 13 == 0 {
            ConnState::Reconnecting {
                attempt: ((frame % 5) + 1) as u32,
                retry_in_s: 1.5,
            }
        } else {
            ConnState::Streaming
        },
        stats,
        spots: Vec::new(),
            decode_channels: Vec::new(),
        rows: vec![latest.clone(); (frame % 4) + 1],
        latest,
        last_error: if frame % 19 == 0 {
            Some("synthetic stall".into())
        } else {
            None
        },
        audio_scope: vec![0.0; 128],
        audio_waveform: Vec::new(),
    }
}

fn flood_spots(n: usize) -> Vec<Spot> {
    let now = Instant::now();
    (0..n)
        .map(|i| Spot {
            frequency_hz: 14_010_000.0 + i as f64 * 125.0,
            callsign: Some(format!("T{i:03}")),
            kind: if i % 3 == 0 {
                SpotKind::CallingCq
            } else if i % 3 == 1 {
                SpotKind::Answering
            } else {
                SpotKind::Heard
            },
            snr_db: 8.0 + (i % 20) as f32,
            wpm: 18.0 + (i % 10) as f32,
            first_heard: now,
            last_heard: now,
            sources: Vec::new(),
            callsign_rank: 0,
        })
        .collect()
}

#[test]
fn rapid_streaming_frames_stay_finite() {
    let mut harness = stress_harness(Vec2::new(1580.0, 960.0), 256);
    harness.run_steps(1);
    for frame in 0..120 {
        inject_and_step(&mut harness, synthetic_streaming_poll(frame), 1);
    }
    assert_ui_finite(harness.state());
    assert!(matches!(
        harness.state().engine_ui.conn_state,
        ConnState::Streaming
    ));
}

#[test]
fn corrupt_poll_sequence_sanitizes() {
    let mut harness = stress_harness(Vec2::new(1580.0, 960.0), 256);
    harness.run_steps(1);
    for frame in 0..64 {
        inject_and_step(&mut harness, corrupt_poll(frame), 1);
    }
    assert_ui_finite(harness.state());
}

#[test]
fn connection_state_machine_perturbation() {
    let mut harness = stress_harness(Vec2::new(1580.0, 960.0), 128);
    harness.run_steps(1);
    let states = [
        ConnState::Disconnected,
        ConnState::Connecting {
            label: "rx.test:8073".into(),
        },
        ConnState::Streaming,
        ConnState::Reconnecting {
            attempt: 2,
            retry_in_s: 3.0,
        },
        ConnState::Disconnected,
    ];
    for (i, state) in states.iter().enumerate() {
        let latest = vec![-90.0; FFT_SIZE];
        harness.state().inject_engine_poll(EnginePoll {
            state: state.clone(),
            stats: streaming_stats(),
            spots: Vec::new(),
            decode_channels: Vec::new(),
            rows: vec![latest.clone()],
            latest,
            last_error: if i == 4 {
                Some("lost link".into())
            } else {
                None
            },
            audio_scope: vec![0.0; 64],
            audio_waveform: Vec::new(),
        });
        harness.run_steps(2);
        assert_ui_finite(harness.state());
    }
}

#[test]
fn all_panels_and_drawers_open_while_streaming() {
    let mut harness = stress_harness(Vec2::new(1580.0, 960.0), 192);
    harness.run_steps(1);
    inject_and_step(&mut harness, synthetic_streaming_poll(0), 2);
    {
        let app = harness.state_mut();
        app.chrome.show_left = true;
        app.chrome.show_right = true;
        app.chrome.show_console = true;
        app.chrome.show_history = true;
        app.chrome.show_af_scope = true;
        app.chrome.show_smeter = true;
        app.chrome.show_iq_drawer = true;
        app.chrome.show_pipeline_drawer = true;
        app.chrome.show_filter_drawer = true;
        app.chrome.show_shortcuts = true;
        app.connection.form.show_connection_drawer = true;
        app.display.show_band_overview = true;
        app.radio.is_kiwi = true;
    }
    for frame in 0..24 {
        inject_and_step(&mut harness, synthetic_streaming_poll(frame), 2);
    }
    assert_ui_finite(harness.state());
}

#[test]
fn panel_toggle_stress_while_streaming() {
    let mut harness = stress_harness(Vec2::new(1580.0, 960.0), 192);
    harness.run_steps(1);
    inject_and_step(&mut harness, synthetic_streaming_poll(0), 2);
    for i in 0..30 {
        {
            let chrome = &mut harness.state_mut().chrome;
            chrome.show_left = i % 2 == 0;
            chrome.show_right = i % 3 != 0;
            chrome.show_console = i % 4 == 0;
            chrome.show_history = i % 5 == 0;
            chrome.show_smeter = true;
            chrome.cw_simple_ui = i % 6 == 0;
        }
        inject_and_step(&mut harness, synthetic_streaming_poll(i), 1);
    }
    assert_ui_finite(harness.state());
}

#[test]
fn layout_minimum_size_renders() {
    let mut harness = stress_harness(Vec2::new(1100.0, 720.0), 96);
    harness.run_steps(3);
    harness.get_by_label("OFFLINE");
    inject_and_step(&mut harness, synthetic_streaming_poll(0), 2);
    harness.get_by_label("STREAMING");
    assert_ui_finite(harness.state());
}

#[test]
fn layout_large_size_renders() {
    let mut harness = stress_harness(Vec2::new(1920.0, 1080.0), 96);
    harness.run_steps(3);
    inject_and_step(&mut harness, synthetic_streaming_poll(0), 2);
    assert_ui_finite(harness.state());
}

#[test]
fn spot_flood_while_streaming() {
    let mut harness = stress_harness(Vec2::new(1580.0, 960.0), 192);
    harness.run_steps(1);
    harness.state_mut().skimmer_ui.skimmer_enabled = true;
    for frame in 0..20 {
        let mut poll = synthetic_streaming_poll(frame);
        poll.spots = flood_spots(80);
        inject_and_step(&mut harness, poll, 2);
    }
    assert_ui_finite(harness.state());
    assert!(!harness.state().skimmer_ui.skimmer_spots.is_empty());
}

#[test]
fn fft_size_change_mid_stream() {
    let mut harness = stress_harness(Vec2::new(1580.0, 960.0), 128);
    harness.run_steps(1);
    for (frame, fft) in [(0, FFT_SIZE), (1, 2048), (2, 4096), (3, 8192), (4, FFT_SIZE)] {
        let latest = vec![-90.0; fft];
        inject_and_step(
            &mut harness,
            EnginePoll {
                state: ConnState::Streaming,
                stats: streaming_stats(),
                spots: Vec::new(),
            decode_channels: Vec::new(),
                rows: vec![latest.clone()],
                latest,
                last_error: None,
                audio_scope: vec![0.0; 64],
                audio_waveform: Vec::new(),
            },
            2,
        );
        assert_eq!(harness.state().plot.latest.len(), fft);
        assert_ui_finite(harness.state());
        let _ = frame;
    }
}

#[test]
fn keyboard_shortcut_barrage() {
    let mut harness = stress_harness(Vec2::new(1580.0, 960.0), 256);
    harness.run_steps(1);
    inject_and_step(&mut harness, synthetic_streaming_poll(0), 2);
    harness.state_mut().radio.lock_ham_bands = false;
    for key in [
        Key::R,
        Key::L,
        Key::A,
        Key::N,
        Key::B,
        Key::OpenBracket,
        Key::CloseBracket,
        Key::F,
        Key::Space,
        Key::Equals,
        Key::Minus,
        Key::Backtick,
        Key::M,
        Key::Questionmark,
    ] {
        harness.key_press(key);
        harness.run_steps(1);
    }
    assert_ui_finite(harness.state());
}

#[test]
fn pipeline_stage_toggle_stress() {
    let mut harness = stress_harness(Vec2::new(1580.0, 960.0), 128);
    harness.run_steps(1);
    inject_and_step(&mut harness, synthetic_streaming_poll(0), 2);
    harness.state_mut().chrome.show_pipeline_drawer = true;
    for stage in [
        PipelineStage::NoiseBlanker,
        PipelineStage::ManualNotches,
        PipelineStage::Agc,
        PipelineStage::Apf,
        PipelineStage::AutoNotch,
        PipelineStage::Skimmer,
        PipelineStage::AudioOutput,
    ] {
        harness.state_mut().toggle_pipeline_stage(stage);
        harness.run_steps(2);
    }
    assert_ui_finite(harness.state());
}

#[test]
fn source_kind_switch_while_offline() {
    let mut harness = stress_harness(Vec2::new(1580.0, 960.0), 96);
    harness.run_steps(1);
    harness.state_mut().connection.form.show_connection_drawer = true;
    for kind in [
        SourceKind::Kiwi,
        SourceKind::Airspy,
        #[cfg(feature = "rtlsdr")]
        SourceKind::RtlSdr,
        #[cfg(feature = "qmx")]
        SourceKind::Qmx,
    ] {
        harness.state_mut().connection.form.kind = kind;
        harness.run_steps(4);
    }
}

#[test]
fn empty_then_full_spectrum_oscillation() {
    let mut harness = stress_harness(Vec2::new(1580.0, 960.0), 128);
    harness.run_steps(1);
    for frame in 0..32 {
        let poll = if frame % 2 == 0 {
            EnginePoll {
                state: ConnState::Streaming,
                stats: streaming_stats(),
                spots: Vec::new(),
            decode_channels: Vec::new(),
                rows: Vec::new(),
                latest: Vec::new(),
                last_error: None,
                audio_scope: Vec::new(),
                audio_waveform: Vec::new(),
            }
        } else {
            synthetic_streaming_poll(frame)
        };
        inject_and_step(&mut harness, poll, 1);
    }
    assert_ui_finite(harness.state());
}

#[test]
fn wideband_stats_perturbation() {
    let mut harness = stress_harness(Vec2::new(1580.0, 960.0), 128);
    harness.run_steps(1);
    harness.state_mut().radio.is_kiwi = false;
    for rate in [96_000.0, 192_000.0, 384_000.0, 768_000.0] {
        let mut stats = streaming_stats();
        stats.sample_rate = rate;
        stats.iq_passband_hz = rate;
        stats.spectrum_rate = rate;
        stats.effective_sps = rate * 0.65;
        stats.slow = rate >= 384_000.0;
        let latest = vec![-90.0; FFT_SIZE];
        inject_and_step(
            &mut harness,
            EnginePoll {
                state: ConnState::Streaming,
                stats,
                spots: Vec::new(),
            decode_channels: Vec::new(),
                rows: vec![latest.clone()],
                latest,
                last_error: None,
                audio_scope: vec![0.0; 128],
                audio_waveform: Vec::new(),
            },
            3,
        );
        assert_ui_finite(harness.state());
    }
}
