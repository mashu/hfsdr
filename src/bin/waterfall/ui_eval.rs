//! UI evaluation harness — verifies layout landmarks and captures reference screenshots.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;

use eframe::egui::Vec2;
use egui_kittest::{Harness, kittest::Queryable as _};

use crate::app::WaterfallApp;
use crate::audio;
use crate::engine::{ConnState, EnginePoll, FFT_SIZE};
use crate::theme;
use crate::ui_smoke::{inject_and_step, streaming_stats, synthetic_streaming_poll};

const TEST_AUDIO_DEVICES: [&str; 1] = ["Test Output"];
const WINDOW_SIZE: Vec2 = Vec2::new(1580.0, 960.0);

fn eval_harness(size: Vec2) -> Harness<'static, WaterfallApp> {
    audio::set_test_output_devices(Some(
        TEST_AUDIO_DEVICES.iter().map(|s| (*s).to_string()).collect(),
    ));
    Harness::builder()
        .with_size(size)
        .with_max_steps(128)
        .with_wait_for_pending_images(false)
        .build_eframe(|cc| {
            theme::apply(&cc.egui_ctx);
            WaterfallApp::new_for_test(None)
        })
}

fn screenshot_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/ui_screenshots");
    std::fs::create_dir_all(&dir).expect("create screenshot dir");
    dir
}

fn save_render(harness: &mut Harness<'_, WaterfallApp>, name: &str) -> Result<(), String> {
    let path = screenshot_dir().join(format!("{name}.png"));
    let image = match catch_unwind(AssertUnwindSafe(|| harness.render())) {
        Ok(result) => result?,
        Err(_) => return Err("wgpu adapter unavailable (headless runner)".into()),
    };
    image.save(&path).map_err(|e| e.to_string())
}

/// Returns false when the runner has no wgpu adapter (typical on headless Linux CI).
fn wgpu_render_available(harness: &mut Harness<'_, WaterfallApp>) -> bool {
    match catch_unwind(AssertUnwindSafe(|| harness.render())) {
        Ok(Ok(_)) => true,
        Ok(Err(err)) => {
            eprintln!("skipping UI screenshot capture: {err}");
            false
        }
        Err(_) => {
            eprintln!("skipping UI screenshot capture: wgpu adapter unavailable (headless runner)");
            false
        }
    }
}

#[test]
fn evaluate_startup_landmarks() {
    let mut harness = eval_harness(WINDOW_SIZE);
    harness.run_steps(4);
    harness.get_by_label("OFFLINE");
    harness.get_by_label("DSP");
    harness.get_by_label("RX");
    assert_eq!(
        harness.state().audio.audio_devices,
        vec!["Test Output".to_string()]
    );
}

#[test]
fn evaluate_streaming_landmarks() {
    let mut harness = eval_harness(WINDOW_SIZE);
    harness.run_steps(1);
    inject_and_step(&mut harness, synthetic_streaming_poll(0), 4);
    harness.get_by_label("STREAMING");
    harness.get_by_label("DSP");
}

#[test]
fn evaluate_connection_drawer_opens() {
    let mut harness = eval_harness(WINDOW_SIZE);
    harness.run_steps(1);
    harness.get_by_label("OFFLINE").click();
    harness.run_steps(4);
    harness.get_by_label("Connection");
}

#[test]
fn evaluate_minimum_window_landmarks() {
    let mut harness = eval_harness(Vec2::new(1100.0, 720.0));
    harness.run_steps(4);
    harness.get_by_label("OFFLINE");
    harness.get_by_label("DSP");
}

#[test]
fn evaluate_reconnecting_badge() {
    let mut harness = eval_harness(WINDOW_SIZE);
    harness.run_steps(1);
    harness.state().inject_engine_poll(EnginePoll {
        state: ConnState::Reconnecting {
            attempt: 1,
            retry_in_s: 2.0,
        },
        stats: streaming_stats(),
        spots: Vec::new(),
            decode_channels: Vec::new(),
        rows: Vec::new(),
        latest: vec![-90.0; FFT_SIZE],
        last_error: None,
        audio_scope: Vec::new(),
        audio_waveform: Vec::new(),
    });
    harness.run_steps(4);
    harness.get_by_label("RECONNECT #1 (2s)");
}

/// wgpu `render()` is not safe to call from parallel tests — keep screenshots in one test.
#[test]
fn capture_ui_screenshot_states() {
    let mut harness = eval_harness(WINDOW_SIZE);
    harness.run_steps(4);
    if !wgpu_render_available(&mut harness) {
        return;
    }
    save_render(&mut harness, "01_startup_offline").expect("write startup screenshot");

    inject_and_step(&mut harness, synthetic_streaming_poll(0), 4);
    save_render(&mut harness, "02_streaming_default").expect("write streaming screenshot");

    {
        let app = harness.state_mut();
        app.chrome.show_left = true;
        app.chrome.show_right = true;
        app.skimmer_ui.skimmer_enabled = true;
    }
    harness.run_steps(8);
    save_render(&mut harness, "03_streaming_full_ui").expect("write full-ui screenshot");

    harness.state_mut().connection.form.show_connection_drawer = true;
    harness.run_steps(4);
    save_render(&mut harness, "04_connection_drawer").expect("write drawer screenshot");

    harness.state().inject_engine_poll(EnginePoll {
        state: ConnState::Reconnecting {
            attempt: 1,
            retry_in_s: 2.0,
        },
        stats: streaming_stats(),
        spots: Vec::new(),
            decode_channels: Vec::new(),
        rows: Vec::new(),
        latest: vec![-90.0; FFT_SIZE],
        last_error: None,
        audio_scope: Vec::new(),
        audio_waveform: Vec::new(),
    });
    harness.run_steps(4);
    save_render(&mut harness, "05_reconnecting").expect("write reconnecting screenshot");

    let mut min_harness = eval_harness(Vec2::new(1100.0, 720.0));
    min_harness.run_steps(4);
    save_render(&mut min_harness, "06_minimum_window").expect("write minimum-window screenshot");
}
