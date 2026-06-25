//! Direct UI method tests — render panel bodies without full-window layout.

use std::time::Instant;

use eframe::egui::Vec2;
use egui_kittest::Harness;

use crate::app::WaterfallApp;
use crate::audio;
use crate::engine::EngineStats;
use crate::iq_panel::IqPanelView;
use crate::skimmer::ScpStatus;
use hfsdr::{Spot, SpotKind};

fn seeded_app() -> WaterfallApp {
    audio::set_test_output_devices(Some(vec!["Test Output".into()]));
    let mut app = WaterfallApp::new_for_test(None);
    app.skimmer_ui.skimmer_enabled = true;
    app.skimmer_ui.skimmer_channels = 3;
    let now = Instant::now();
    app.skimmer_ui.frame_visible_spots = vec![
        Spot {
            frequency_hz: 14_010_500.0,
            callsign: Some("G0ABC".into()),
            kind: SpotKind::CallingCq,
            snr_db: 20.0,
            wpm: 22.0,
            first_heard: now,
            last_heard: now,
            sources: Vec::new(),
            callsign_rank: 0,
        },
        Spot {
            frequency_hz: 14_011_000.0,
            callsign: None,
            kind: SpotKind::Heard,
            snr_db: 12.0,
            wpm: 18.0,
            first_heard: now,
            last_heard: now,
            sources: Vec::new(),
            callsign_rank: 0,
        },
    ];
    app.skimmer_ui.skimmer_spots = app.skimmer_ui.frame_visible_spots.clone();
    app.engine_ui.stats = EngineStats {
        sample_rate: 96_000.0,
        iq_passband_hz: 96_000.0,
        spectrum_rate: 96_000.0,
        spectrum_fft: 4096,
        audio_device: Some("Test Output".into()),
        audio_rate: 48_000,
        scp: ScpStatus {
            loaded: true,
            calls: 5000,
            version: Some("test".into()),
            path: Some("/tmp/MASTER.SCP".into()),
        },
        ..EngineStats::default()
    };
    app
}

fn run_ui<F>(app: WaterfallApp, mut draw: F)
where
    F: FnMut(&mut WaterfallApp, &mut eframe::egui::Ui) + 'static,
{
    let mut harness = Harness::builder()
        .with_size(Vec2::new(420.0, 900.0))
        .with_max_steps(24)
        .with_wait_for_pending_images(false)
        .build_ui_state(move |ui, app: &mut WaterfallApp| draw(app, ui), app);
    harness.run_steps(8);
}

#[test]
fn spot_display_body_renders() {
    let app = seeded_app();
    run_ui(app, |app, ui| app.spot_display_body(ui));
}

#[test]
fn spot_display_section_renders() {
    let app = seeded_app();
    run_ui(app, |app, ui| app.spot_display_section(ui));
}

#[test]
fn skimmer_settings_body_renders() {
    let app = seeded_app();
    run_ui(app, |app, ui| app.skimmer_settings_body(ui));
}

#[test]
fn scp_body_renders_loaded_and_empty() {
    let app = seeded_app();
    run_ui(app, |app, ui| app.scp_body(ui));

    let mut empty = seeded_app();
    empty.engine_ui.stats.scp = ScpStatus::default();
    run_ui(empty, |app, ui| app.scp_body(ui));
}

#[test]
fn audio_card_body_renders() {
    let app = seeded_app();
    run_ui(app, |app, ui| app.audio_card_body(ui));

    let mut no_dev = seeded_app();
    no_dev.audio.audio_devices.clear();
    run_ui(no_dev, |app, ui| app.audio_card_body(ui));
}

#[test]
fn performance_section_renders() {
    let app = seeded_app();
    run_ui(app, |app, ui| app.performance_section(ui));

    let mut wide = seeded_app();
    wide.radio.is_kiwi = false;
    wide.engine_ui.stats.iq_passband_hz = 384_000.0;
    wide.engine_ui.stats.sample_rate = 384_000.0;
    wide.engine_ui.stats.spectrum_rate = 384_000.0;
    wide.display.fft_auto = false;
    run_ui(wide, |app, ui| app.performance_section(ui));
}

#[test]
fn history_panel_with_annotations() {
    let mut app = seeded_app();
    app.annotate_new_spots(14_010_000.0);
    run_ui(app, |app, ui| app.history_panel(ui));
}

#[test]
fn history_panel_empty_state() {
    let app = seeded_app();
    run_ui(app, |app, ui| app.history_panel(ui));
}

#[test]
fn iq_panel_show_renders() {
    let app = seeded_app();
    run_ui(app, |app, ui| {
        let view = IqPanelView {
            stats: &app.engine_ui.stats,
            streaming: true,
        };
        let _ = app.chrome.iq.show(ui, view);
    });
}

#[test]
fn display_section_renders() {
    let app = seeded_app();
    run_ui(app, |app, ui| app.display_section(ui));
}

#[test]
fn spot_display_airspy_skimmer_warning() {
    let mut app = seeded_app();
    app.radio.is_kiwi = false;
    app.engine_ui.stats.sample_rate = 768_000.0;
    run_ui(app, |app, ui| app.spot_display_body(ui));
}

#[test]
fn cw_demod_and_af_tuning_cards_render() {
    let app = seeded_app();
    run_ui(app, |app, ui| {
        app.af_tuning_card(ui);
        app.cw_demod_card(ui);
    });
}

#[test]
fn left_panel_cards_render() {
    let app = seeded_app();
    run_ui(app, |app, ui| {
        app.smeter_card(ui);
        app.frequency_card(ui);
        app.rf_front_end_card(ui);
        app.receive_chain_card(ui);
    });
}

#[test]
fn connection_sections_render() {
    let mut app = seeded_app();
    app.connection.form.show_connection_drawer = true;
    run_ui(app, |app, ui| {
        app.connection_card(ui);
        app.connection_form_section(ui);
        app.connection_kiwi_iq_section(ui);
        app.connection_kiwi_browser_section(ui);
        app.connection_recent_section(ui);
        #[cfg(feature = "airspy")]
        app.connection_airspy_section(ui);
        #[cfg(feature = "rtlsdr")]
        app.connection_rtlsdr_section(ui);
        #[cfg(feature = "qmx")]
        app.connection_qmx_section(ui);
    });
}

#[test]
fn all_popups_via_full_app() {
    use eframe::egui::Vec2;
    use egui_kittest::Harness;
    use crate::theme;
    use crate::ui_smoke::{inject_and_step, synthetic_streaming_poll};

    audio::set_test_output_devices(Some(vec!["Test Output".into()]));
    let mut harness = Harness::builder()
        .with_size(Vec2::new(1580.0, 960.0))
        .with_max_steps(48)
        .build_eframe(|cc| {
            theme::apply(&cc.egui_ctx);
            WaterfallApp::new_for_test(None)
        });
    harness.run_steps(1);
    inject_and_step(&mut harness, synthetic_streaming_poll(0), 2);
    harness.state_mut().connection.form.show_connection_drawer = true;
    harness.state_mut().chrome.show_iq_drawer = true;
    harness.state_mut().chrome.show_pipeline_drawer = true;
    harness.state_mut().chrome.show_shortcuts = true;
    harness.run_steps(12);
}
