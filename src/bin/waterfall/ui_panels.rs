//! Headless panel / drawer smoke tests — exercise egui chrome not covered by ui_smoke.

use std::time::Instant;

use eframe::egui::Vec2;
use egui_kittest::{Harness, kittest::Queryable as _};

use crate::app::WaterfallApp;
use crate::audio;
use crate::engine::{ConnState, EnginePoll, EngineStats, FFT_SIZE};
use crate::pipeline_flow::PipelineStage;
use crate::skimmer::ScpStatus;
use crate::source::SourceKind;
use crate::theme;
use crate::ui_smoke::{inject_and_step, streaming_stats, synthetic_streaming_poll};
use hfsdr::{Spot, SpotKind};

const TEST_AUDIO_DEVICES: [&str; 1] = ["Test Output"];

fn panel_harness() -> Harness<'static, WaterfallApp> {
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

fn streaming_harness() -> Harness<'static, WaterfallApp> {
    let mut harness = panel_harness();
    harness.run_steps(1);
    inject_and_step(&mut harness, synthetic_streaming_poll(0), 2);
    harness
}

fn right_panel_harness() -> Harness<'static, WaterfallApp> {
    let mut harness = streaming_harness();
    harness.state_mut().chrome.show_right = true;
    harness
}

fn sample_spot(call: &str, kind: SpotKind) -> Spot {
    let now = Instant::now();
    Spot {
        frequency_hz: 14_010_500.0,
        callsign: Some(call.into()),
        kind,
        snr_db: 18.0,
        wpm: 22.0,
        first_heard: now,
        last_heard: now,
        sources: Vec::new(),
        callsign_rank: 0,
    }
}

fn poll_with_spots() -> EnginePoll {
    let latest = vec![-90.0; FFT_SIZE];
    EnginePoll {
        state: ConnState::Streaming,
        stats: streaming_stats(),
        spots: vec![sample_spot("TEST1", SpotKind::CallingCq)],
        rows: vec![latest.clone()],
        latest,
        last_error: None,
        audio_scope: vec![0.0; 128],
    }
}

fn poll_with_scp_loaded() -> EnginePoll {
    let mut poll = synthetic_streaming_poll(0);
    poll.stats.scp = ScpStatus {
        loaded: true,
        calls: 12_345,
        version: Some("2024-01".into()),
        path: Some("/home/user/.config/hfsdr/MASTER.SCP".into()),
    };
    poll
}

fn click_by_label(harness: &mut Harness<'_, WaterfallApp>, label: &str) {
    let node = harness
        .get_all_by_label(label)
        .last()
        .unwrap_or_else(|| panic!("no node with label {label:?}"));
    node.click();
}

fn open_right_collapsibles(harness: &mut Harness<'_, WaterfallApp>) {
    harness.run_steps(2);
    for label in ["Spots", "Skimmer settings", "Audio", "Performance"] {
        click_by_label(harness, label);
    }
}

fn seed_spot_table(harness: &mut Harness<'_, WaterfallApp>) {
    harness.state_mut().skimmer_ui.skimmer_enabled = true;
    harness.state_mut().skimmer_ui.frame_visible_spots = vec![
        sample_spot("G0ABC", SpotKind::CallingCq),
        sample_spot("DL1TEST", SpotKind::Answering),
        Spot {
            callsign: None,
            kind: SpotKind::Heard,
            ..sample_spot("", SpotKind::Heard)
        },
    ];
}

#[test]
fn console_and_history_panels_render() {
    let mut harness = streaming_harness();
    harness.state_mut().chrome.show_console = true;
    harness.state_mut().chrome.show_history = true;
    inject_and_step(&mut harness, poll_with_spots(), 6);
    assert!(harness.state().chrome.show_console);
    assert!(harness.state().chrome.show_history);
}

#[test]
fn panel_toggles_dsp_rx_scope_meter() {
    let mut harness = streaming_harness();
    harness.get_by_label("DSP").click();
    harness.get_by_label("RX").click();
    harness.get_by_label("Scope").click();
    harness.get_by_label("Meter").click();
    harness.run_steps(4);

    let chrome = &harness.state().chrome;
    assert!(!chrome.show_right);
    assert!(!chrome.show_left);
    assert!(!chrome.show_af_scope);
    assert!(!chrome.show_smeter);
}

#[test]
fn pipeline_drawer_renders_while_streaming() {
    let mut harness = streaming_harness();
    harness.state_mut().chrome.show_pipeline_drawer = true;
    harness.run_steps(6);
    assert!(harness.state().chrome.show_pipeline_drawer);
}

#[test]
fn iq_drawer_renders_while_streaming() {
    let mut harness = streaming_harness();
    harness.state_mut().chrome.show_iq_drawer = true;
    harness.run_steps(6);
    assert!(harness.state().chrome.show_iq_drawer);
}

#[test]
fn shortcuts_popup_renders() {
    let mut harness = streaming_harness();
    harness.state_mut().chrome.show_shortcuts = true;
    harness.run_steps(4);
    assert!(harness.state().chrome.show_shortcuts);
}

#[test]
fn connection_drawer_renders() {
    let mut harness = panel_harness();
    harness.run_steps(1);
    harness.state_mut().connection.form.show_connection_drawer = true;
    harness.run_steps(6);
    assert!(harness.state().connection.form.show_connection_drawer);
}

#[test]
fn history_panel_with_spots_poll() {
    let mut harness = streaming_harness();
    harness.state_mut().chrome.show_history = true;
    inject_and_step(&mut harness, poll_with_spots(), 4);
    assert!(harness.state().chrome.show_history);
}

#[test]
fn right_panel_skimmer_section_renders() {
    let mut harness = streaming_harness();
    harness.state_mut().chrome.show_right = true;
    harness.state_mut().skimmer_ui.skimmer_enabled = true;
    harness.run_steps(8);
    assert!(harness.state().skimmer_ui.skimmer_enabled);
}

#[test]
fn right_panel_collapsibles_render() {
    let mut harness = streaming_harness();
    harness.state_mut().chrome.show_right = true;
    harness.state_mut().skimmer_ui.skimmer_enabled = true;
    harness.run_steps(4);
    for label in ["Skimmer settings", "Audio", "Performance"] {
        harness.get_by_label(label).click();
    }
    harness.run_steps(8);
}

#[test]
fn kiwi_band_overview_renders() {
    let mut harness = streaming_harness();
    harness.state_mut().radio.is_kiwi = true;
    harness.state_mut().display.show_band_overview = true;
    harness.run_steps(8);
    assert!(harness.state().display.show_band_overview);
}

#[test]
fn pipeline_drawer_toggles_stage() {
    let mut harness = streaming_harness();
    harness.state_mut().chrome.show_pipeline_drawer = true;
    harness.run_steps(4);
    let before = harness.state().radio.cw.agc.enabled;
    harness.state_mut().toggle_pipeline_stage(PipelineStage::Agc);
    assert_ne!(harness.state().radio.cw.agc.enabled, before);
}

#[test]
fn connection_form_airspy_kind_renders() {
    let mut harness = panel_harness();
    harness.run_steps(1);
    harness.state_mut().connection.form.kind = SourceKind::Airspy;
    harness.state_mut().connection.form.show_connection_drawer = true;
    harness.run_steps(6);
    assert_eq!(harness.state().connection.form.kind, SourceKind::Airspy);
}

#[test]
fn connection_form_rtlsdr_kind_renders() {
    let mut harness = panel_harness();
    harness.run_steps(1);
    harness.state_mut().connection.form.kind = SourceKind::RtlSdr;
    harness.state_mut().connection.form.show_connection_drawer = true;
    harness.run_steps(6);
    assert_eq!(harness.state().connection.form.kind, SourceKind::RtlSdr);
}

#[test]
fn connection_form_qmx_kind_renders() {
    let mut harness = panel_harness();
    harness.run_steps(1);
    harness.state_mut().connection.form.kind = SourceKind::Qmx;
    harness.state_mut().connection.form.show_connection_drawer = true;
    harness.run_steps(6);
    assert_eq!(harness.state().connection.form.kind, SourceKind::Qmx);
}

#[test]
fn spots_panel_table_and_filters_render() {
    let mut harness = right_panel_harness();
    seed_spot_table(&mut harness);
    open_right_collapsibles(&mut harness);
    harness.run_steps(12);
    assert!(harness.state().skimmer_ui.skimmer_enabled);
}

#[test]
fn spots_panel_continent_filter_and_cq_only() {
    let mut harness = right_panel_harness();
    seed_spot_table(&mut harness);
    open_right_collapsibles(&mut harness);
    harness.state_mut().skimmer_ui.spot_cq_only = true;
    harness.state_mut().skimmer_ui.continent_filter = true;
    harness.run_steps(12);
    assert!(harness.state().skimmer_ui.spot_cq_only);
    assert!(harness.state().skimmer_ui.continent_filter);
}

#[test]
fn skimmer_settings_all_sections_render() {
    let mut harness = right_panel_harness();
    harness.state_mut().skimmer_ui.skimmer_enabled = true;
    harness.state_mut().skimmer_ui.skimmer_channels = 4;
    open_right_collapsibles(&mut harness);
    harness.run_steps(12);
}

#[test]
fn audio_panel_device_and_playback_controls() {
    let mut harness = right_panel_harness();
    open_right_collapsibles(&mut harness);
    harness.run_steps(12);
}

#[test]
fn audio_panel_shows_active_device_when_streaming() {
    let mut harness = right_panel_harness();
    let mut stats = streaming_stats();
    stats.audio_device = Some("Test Output".into());
    stats.audio_rate = 48_000;
    inject_and_step(&mut harness, poll_with_stats(stats), 2);
    open_right_collapsibles(&mut harness);
    harness.run_steps(6);
}

#[test]
fn performance_panel_fft_and_decimation_controls() {
    let mut harness = right_panel_harness();
    harness.state_mut().display.fft_auto = false;
    harness.state_mut().display.fft_size = 4096;
    open_right_collapsibles(&mut harness);
    harness.run_steps(12);
    assert!(!harness.state().display.fft_auto);
}

#[test]
fn performance_panel_wideband_skimmer_caps() {
    let mut harness = right_panel_harness();
    harness.state_mut().radio.is_kiwi = false;
    harness.state_mut().skimmer_ui.skimmer_enabled = true;
    harness.state_mut().engine_ui.stats.iq_passband_hz = 384_000.0;
    harness.state_mut().engine_ui.stats.sample_rate = 384_000.0;
    harness.state_mut().engine_ui.stats.spectrum_rate = 384_000.0;
    harness.state_mut().engine_ui.stats.spectrum_fft = FFT_SIZE;
    open_right_collapsibles(&mut harness);
    harness.run_steps(8);
}

#[test]
fn scp_panel_loaded_and_reload() {
    let mut harness = right_panel_harness();
    inject_and_step(&mut harness, poll_with_scp_loaded(), 2);
    open_right_collapsibles(&mut harness);
    harness.run_steps(12);
    assert!(harness.state().engine_ui.stats.scp.loaded);
}

#[test]
fn scp_panel_not_loaded_shows_warning() {
    let mut harness = right_panel_harness();
    open_right_collapsibles(&mut harness);
    harness.run_steps(6);
}

#[test]
fn left_panel_rf_cards_render() {
    let mut harness = streaming_harness();
    harness.state_mut().chrome.show_left = true;
    harness.state_mut().chrome.show_smeter = true;
    harness.run_steps(8);
}

#[test]
fn status_bar_log_and_history_toggles() {
    let mut harness = streaming_harness();
    harness.state_mut().chrome.show_console = true;
    harness.state_mut().chrome.show_history = true;
    harness.run_steps(8);
    assert!(harness.state().chrome.show_console);
    assert!(harness.state().chrome.show_history);
}

#[test]
fn history_panel_with_annotations_from_spots() {
    let mut harness = streaming_harness();
    harness.state_mut().chrome.show_history = true;
    harness.state_mut().skimmer_ui.skimmer_enabled = true;
    inject_and_step(&mut harness, poll_with_spots(), 4);
    harness.run_steps(6);
}

#[test]
fn iq_drawer_record_controls_while_streaming() {
    let mut harness = streaming_harness();
    harness.state_mut().chrome.show_iq_drawer = true;
    harness.run_steps(6);
    click_by_label(&mut harness, "Record");
    harness.run_steps(4);
}

#[test]
fn status_widgets_chips_render_while_streaming() {
    let mut harness = streaming_harness();
    harness.state_mut().engine_ui.stats.iq_buffer_fill = 0.42;
    harness.run_steps(8);
}

#[test]
fn spots_airspy_wideband_warning_when_skimmer_on() {
    let mut harness = right_panel_harness();
    harness.state_mut().radio.is_kiwi = false;
    harness.state_mut().skimmer_ui.skimmer_enabled = true;
    harness.state_mut().engine_ui.stats.sample_rate = 768_000.0;
    open_right_collapsibles(&mut harness);
    harness.run_steps(8);
}

#[test]
fn connection_form_kiwi_browser_renders() {
    let mut harness = panel_harness();
    harness.run_steps(1);
    harness.state_mut().connection.form.kind = SourceKind::Kiwi;
    harness.state_mut().connection.form.show_connection_drawer = true;
    harness.run_steps(8);
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
