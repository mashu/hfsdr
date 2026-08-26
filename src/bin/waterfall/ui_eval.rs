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
        rows: Vec::new(),
        latest: vec![-90.0; FFT_SIZE],
        last_error: None,
        audio_scope: Vec::new(),
        audio_waveform: Vec::new(),
    });
    harness.run_steps(4);
    harness.get_by_label("RECONNECT #1 (2s)");
}


/// End-to-end check of the shader waterfall inside the real app.
///
/// The unit tests in `widgets::waterfall_gpu` prove the shader is correct in
/// isolation; this proves the app actually reaches it — renderer installed,
/// rows queued, callback painted — which the other UI tests do not, since they
/// never set `gpu_available` and so always exercise the CPU path.
#[test]
fn gpu_waterfall_paints_in_the_real_app() {
    audio::set_test_output_devices(Some(
        TEST_AUDIO_DEVICES.iter().map(|s| (*s).to_string()).collect(),
    ));
    let mut installed = false;
    let mut harness = Harness::builder()
        .with_size(WINDOW_SIZE)
        .with_max_steps(128)
        .with_wait_for_pending_images(false)
        // The default test renderer leaves cc.wgpu_render_state empty, which
        // would silently skip this whole test.
        .wgpu()
        .build_eframe(|cc| {
            theme::apply(&cc.egui_ctx);
            installed = crate::widgets::install_waterfall_gpu(cc);
            let mut app = WaterfallApp::new_for_test(None);
            app.set_waterfall_gpu_available(installed);
            app
        });

    if !installed {
        eprintln!("skipping GPU waterfall check: no wgpu render state");
        return;
    }
    harness.run_steps(1);

    // Feed rows with a strong carrier so the result cannot be uniform noise.
    for _ in 0..8 {
        let mut row = vec![-110.0f32; FFT_SIZE];
        row[FFT_SIZE / 2] = -15.0;
        harness.state().inject_engine_poll(EnginePoll {
            state: ConnState::Streaming,
            stats: streaming_stats(),
            rows: vec![row.clone()],
            latest: row,
            last_error: None,
            audio_scope: Vec::new(),
            audio_waveform: Vec::new(),
        });
        harness.run_steps(2);
    }

    let image = match catch_unwind(AssertUnwindSafe(|| harness.render())) {
        Ok(Ok(img)) => img,
        _ => {
            eprintln!("skipping GPU waterfall check: no wgpu adapter");
            return;
        }
    };
    let path = screenshot_dir().join("07_gpu_waterfall.png");
    let _ = image.save(&path);

    // The waterfall occupies the lower half of the plot area. The injected rows
    // are a -110 dB floor with one -15 dB carrier, so against ref -20 / range 80
    // the row must be mostly DARK with a single bright column. Checking only for
    // "several distinct colours" would pass on a solid white texture, which is
    // exactly what an uncleared R32Float ring produces.
    let (w, h) = (image.width(), image.height());
    let band_y = (h as f32 * 0.70) as u32;
    let y = band_y.min(h - 1);
    let mut sums: Vec<u32> = Vec::with_capacity(w as usize);
    for x in 0..w {
        let p = image.get_pixel(x, y);
        sums.push(p[0] as u32 + p[1] as u32 + p[2] as u32);
    }
    let brightest = sums.iter().copied().max().unwrap_or(0);
    let mut sorted = sums.clone();
    sorted.sort_unstable();
    let median = sorted[sorted.len() / 2];

    assert!(
        median < 220,
        "waterfall row is mostly bright (median channel sum {median}) — the dB ring \
         is probably uncleared, which reads as 0 dB = full scale"
    );
    assert!(
        brightest > median + 100,
        "no carrier stands out of the noise floor (max {brightest}, median {median})"
    );
}
