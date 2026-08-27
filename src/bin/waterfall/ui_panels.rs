//! Headless panel / drawer smoke tests — exercise egui chrome not covered by ui_smoke.

use hfsdr::time::Instant;

use eframe::egui::Vec2;
use egui_kittest::{Harness, kittest::Queryable as _};

use crate::app::WaterfallApp;
use crate::audio;
use crate::engine::{ConnState, EnginePoll, EngineStats, FFT_SIZE};
use crate::pipeline_flow::PipelineStage;
use crate::kiwi_directory::KiwiReceiver;
use crate::source::SourceKind;
use crate::theme;
use crate::ui_smoke::{inject_and_step, streaming_stats, synthetic_streaming_poll};


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


fn poll_with_spots() -> EnginePoll {
    let latest = vec![-90.0; FFT_SIZE];
    EnginePoll {
        state: ConnState::Streaming,
        stats: streaming_stats(),
        rows: vec![latest.clone()],
        latest,
        last_error: None,
        audio_scope: vec![0.0; 128],
        audio_waveform: Vec::new(),
    }
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
    for label in ["Audio", "Performance"] {
        click_by_label(harness, label);
    }
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

#[cfg(feature = "airspy")]
#[test]
fn connection_form_airspy_kind_renders() {
    let mut harness = panel_harness();
    harness.run_steps(1);
    harness.state_mut().connection.form.kind = SourceKind::Airspy;
    harness.state_mut().connection.form.show_connection_drawer = true;
    harness.run_steps(6);
    assert_eq!(harness.state().connection.form.kind, SourceKind::Airspy);
}

#[cfg(feature = "rtlsdr")]
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
fn connection_form_kiwi_browser_renders() {
    let mut harness = panel_harness();
    harness.run_steps(1);
    harness.state_mut().connection.form.kind = SourceKind::Kiwi;
    harness.state_mut().connection.form.show_connection_drawer = true;
    harness.run_steps(8);
}

fn directory_entry(host: &str, tls: bool, users: u8) -> KiwiReceiver {
    KiwiReceiver {
        host: host.into(),
        port: if tls { 443 } else { 8073 },
        name: host.into(),
        location: "Testville".into(),
        lat: 0.0,
        lon: 0.0,
        users,
        users_max: 4,
        snr: 30,
        distance_km: 12.0,
        tls,
    }
}

/// The empty-list case above renders a Refresh button and nothing else, so the
/// rows — the reachability labelling, the disabled styling, the ordering — are
/// never drawn. Populate the list so the branch that does the work runs, on
/// both page schemes and with the all-unreachable banner showing.
///
/// A smoke test, deliberately: `list_row` paints its text rather than emitting
/// a widget, so the strings never reach the accessibility tree and cannot be
/// queried back. What the rows say is pinned by the `receiver_line` and
/// `sort_for_display` tests; what this adds is that the section drives them
/// over a real list without panicking, which the empty-list case cannot show.
#[test]
fn connection_form_kiwi_browser_renders_receiver_rows() {
    let lists = [
        // Mixed: reachable, unreachable, and full rows in one pass.
        vec![
            directory_entry("plain.example", false, 0),
            directory_entry("secure.example", true, 0),
            directory_entry("busy.example", true, 4),
        ],
        // All plain http, which on an https page shows the banner instead.
        vec![
            directory_entry("a.example", false, 0),
            directory_entry("b.example", false, 0),
        ],
    ];
    for nearby in lists {
        let mut harness = panel_harness();
        harness.run_steps(1);
        harness.state_mut().connection.form.kind = SourceKind::Kiwi;
        harness.state_mut().connection.form.show_connection_drawer = true;
        harness.state_mut().connection.kiwi.nearby = nearby;
        harness.run_steps(8);

        // Drawing the list must not connect to anything: the rows act on a
        // click, and a section that connects merely by being shown would take
        // the user somewhere they never asked to go.
        assert!(
            harness.state().connection.form.host.is_empty(),
            "rendering the receiver list must not start a connection"
        );
    }
}

fn poll_with_stats(stats: EngineStats) -> EnginePoll {
    let latest = vec![-90.0; FFT_SIZE];
    EnginePoll {
        state: ConnState::Streaming,
        stats,
        rows: vec![latest.clone()],
        latest,
        last_error: None,
        audio_scope: vec![0.0; 128],
        audio_waveform: Vec::new(),
    }
}
