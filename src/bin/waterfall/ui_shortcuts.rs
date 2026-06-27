//! Headless keyboard shortcut smoke tests.

use eframe::egui::{Key, Vec2};
use egui_kittest::Harness;

use crate::app::WaterfallApp;
use crate::audio;
use crate::theme;
use crate::ui_smoke::{inject_and_step, synthetic_streaming_poll};
use crate::engine::FFT_SIZE;
use hfsdr::skimmer::peaks::offset_hz_to_bin;

const TEST_AUDIO_DEVICES: [&str; 1] = ["Test Output"];

fn shortcut_harness() -> Harness<'static, WaterfallApp> {
    audio::set_test_output_devices(Some(
        TEST_AUDIO_DEVICES.iter().map(|s| (*s).to_string()).collect(),
    ));
    Harness::builder()
        .with_size(Vec2::new(1580.0, 960.0))
        .with_max_steps(96)
        .with_wait_for_pending_images(false)
        .build_eframe(|cc| {
            theme::apply(&cc.egui_ctx);
            WaterfallApp::new_for_test(None)
        })
}

fn streaming_shortcut_harness() -> Harness<'static, WaterfallApp> {
    let mut harness = shortcut_harness();
    harness.run_steps(1);
    inject_and_step(&mut harness, synthetic_streaming_poll(0), 2);
    harness
}

#[test]
fn zero_beat_shortcut() {
    let mut harness = streaming_shortcut_harness();
    harness.state_mut().radio.lock_ham_bands = false;
    harness.state_mut().radio.rit_hz = 120.0;
    harness.state_mut().radio.rit_on = true;
    let mut poll = synthetic_streaming_poll(0);
    let bin = offset_hz_to_bin(400.0, FFT_SIZE, 96_000.0);
    poll.latest[bin] = -35.0;
    inject_and_step(&mut harness, poll, 2);
    harness.key_press(Key::Z);
    harness.run_steps(2);
    assert_eq!(harness.state().radio.rit_hz, 0.0);
    assert!(!harness.state().radio.rit_on);
}

#[test]
fn pitch_lock_shortcut_toggles() {
    let mut harness = streaming_shortcut_harness();
    assert!(!harness.state().radio.pitch_lock);
    harness.key_press(Key::L);
    harness.run_steps(2);
    assert!(harness.state().radio.pitch_lock);
}

#[test]
fn rit_toggle_shortcut() {
    let mut harness = streaming_shortcut_harness();
    assert!(!harness.state().radio.rit_on);
    harness.key_press(Key::R);
    harness.run_steps(2);
    assert!(harness.state().radio.rit_on);
    harness.key_press(Key::R);
    harness.run_steps(2);
    assert!(!harness.state().radio.rit_on);
}

#[test]
fn dsp_shortcuts_toggle_stages() {
    let mut harness = streaming_shortcut_harness();
    let agc_before = harness.state().radio.cw.agc.enabled;
    harness.key_press(Key::A);
    harness.run_steps(2);
    assert_ne!(harness.state().radio.cw.agc.enabled, agc_before);

    let notch_before = harness.state().radio.cw.auto_notch.enabled;
    harness.key_press(Key::N);
    harness.run_steps(2);
    assert_ne!(harness.state().radio.cw.auto_notch.enabled, notch_before);

    let blank_before = harness.state().radio.cw.noise_blanker.enabled;
    harness.key_press(Key::B);
    harness.run_steps(2);
    assert_ne!(harness.state().radio.cw.noise_blanker.enabled, blank_before);
}

#[test]
fn passband_narrow_widen_shortcuts() {
    let mut harness = streaming_shortcut_harness();
    let before = harness.state().radio.cw.passband_hz;
    harness.key_press(Key::OpenBracket);
    harness.run_steps(2);
    assert!(harness.state().radio.cw.passband_hz <= before);

    harness.key_press(Key::CloseBracket);
    harness.run_steps(2);
    assert!(harness.state().radio.cw.passband_hz >= before);
}

#[test]
fn rit_shortcuts_adjust_offset() {
    let mut harness = streaming_shortcut_harness();
    harness.state_mut().radio.rit_hz = 0.0;
    harness.key_press(Key::Comma);
    harness.run_steps(2);
    assert!(harness.state().radio.rit_hz < 0.0);
    assert!(harness.state().radio.rit_on);

    harness.key_press(Key::Period);
    harness.run_steps(2);
    assert!(harness.state().radio.rit_hz >= 0.0);

    harness.key_press(Key::Backslash);
    harness.run_steps(2);
    assert_eq!(harness.state().radio.rit_hz, 0.0);
    assert!(!harness.state().radio.rit_on);
}

#[test]
fn view_full_span_shortcut() {
    let mut harness = streaming_shortcut_harness();
    harness.state_mut().plot.plot_view.zoom = 0.25;
    harness.key_press(Key::F);
    harness.run_steps(2);
    assert!((harness.state().plot.plot_view.zoom - 1.0).abs() < 1e-5);
}

#[test]
fn mute_and_volume_shortcuts() {
    let mut harness = streaming_shortcut_harness();
    harness.state_mut().audio.audio_enabled = true;
    harness.key_press(Key::Space);
    harness.run_steps(2);
    assert!(!harness.state().audio.audio_enabled);

    let vol = harness.state().audio.volume;
    harness.key_press(Key::Equals);
    harness.run_steps(2);
    assert!(harness.state().audio.volume >= vol);

    harness.key_press(Key::Minus);
    harness.run_steps(2);
}

#[test]
fn console_and_overview_shortcuts() {
    let mut harness = streaming_shortcut_harness();
    harness.state_mut().radio.is_kiwi = true;
    let console_before = harness.state().chrome.show_console;
    harness.key_press(Key::Backtick);
    harness.run_steps(2);
    assert_ne!(harness.state().chrome.show_console, console_before);

    let overview_before = harness.state().display.show_band_overview;
    harness.key_press(Key::M);
    harness.run_steps(2);
    assert_ne!(harness.state().display.show_band_overview, overview_before);
}

#[test]
fn af_scope_shortcut_opens_right_panel() {
    let mut harness = streaming_shortcut_harness();
    harness.state_mut().chrome.show_right = false;
    harness.state_mut().chrome.show_af_scope = false;
    harness.run_steps(1);
    harness.key_press(Key::G);
    harness.run_steps(4);
    assert!(harness.state().chrome.show_af_scope);
    assert!(harness.state().chrome.show_right);
}

#[test]
fn manual_notch_shortcuts_arm_slots() {
    let mut harness = streaming_shortcut_harness();
    for key in [Key::Num1, Key::Num2, Key::Num3, Key::Num4] {
        harness.key_press(key);
        harness.run_steps(1);
    }
    harness.run_steps(2);
}

#[test]
fn arrow_keys_pan_when_zoomed() {
    let mut harness = streaming_shortcut_harness();
    harness.state_mut().plot.plot_view.zoom = 0.25;
    let before = harness.state().plot.plot_view.pan_offset_hz;
    harness.key_down(Key::ArrowRight);
    harness.run_steps(2);
    harness.key_up(Key::ArrowRight);
    harness.run_steps(1);
    assert_ne!(harness.state().plot.plot_view.pan_offset_hz, before);
}

#[test]
fn shortcuts_popup_toggle() {
    let mut harness = streaming_shortcut_harness();
    harness.key_press(Key::Questionmark);
    harness.run_steps(2);
    assert!(harness.state().chrome.show_shortcuts);
}
