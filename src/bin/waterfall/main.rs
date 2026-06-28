//! Live waterfall + spectrum UI over any [`IqSource`], on egui's wgpu backend.
//!
//! Usage:
//!   waterfall airspy [sample_rate_hz] [center_hz] [process_hz]
//!   waterfall rtlsdr [sample_rate_hz] [center_hz] [process_hz]
//!   waterfall soapy <device_args> [sample_rate_hz] [center_hz] [process_hz]
//!   waterfall qmx [center_hz] [process_hz] [serial_port]  (requires `qmx` feature)
//!   waterfall kiwi <host> [port] [center_hz]

mod meters;
mod app;
mod audio;
mod colormap;
mod controls;
mod display_levels;
mod engine;
mod ham_bands;
mod interaction;
mod iq_panel;
mod kiwi_directory;
mod filter_diagnostic;
mod envelope_diagnostic;
mod pipeline_flow;
mod log;
mod scp_fetch;
mod popup;
mod rf_view;
mod settings;
mod skimmer;
mod source;
mod spot_filter;
mod status_widgets;
mod theme;
mod widgets;
mod waterfall_perf;

#[cfg(test)]
mod ui_smoke;

#[cfg(test)]
mod ui_panels;

#[cfg(test)]
mod app_logic_tests;

#[cfg(test)]
mod ui_shortcuts;

#[cfg(test)]
mod ui_direct;

#[cfg(test)]
mod ui_stress;

#[cfg(test)]
mod ui_eval;

use app::WaterfallApp;
use eframe::egui;

fn main() -> eframe::Result {
    log::init();
    hfsdr::native_sdr::init();
    log_native_sdr_availability();
    log::info("hfsdr starting");
    // The source is no longer built here: the GUI opens immediately and the
    // engine thread connects (auto-connecting if CLI args were supplied), so a
    // missing or slow front end never blocks or crashes the app.
    let autoconnect = source::request_from_args().and_then(|req| {
        if source::source_kind_available(req.kind) {
            Some(req)
        } else {
            log::warn(format!(
                "CLI auto-connect to {} skipped: native driver library not found",
                req.kind
            ));
            None
        }
    });

    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1580.0, 960.0])
            .with_min_inner_size([1100.0, 720.0])
            .with_title("hfsdr"),
        ..Default::default()
    };
    eframe::run_native(
        "hfsdr",
        options,
        Box::new(move |cc| {
            crate::theme::apply(&cc.egui_ctx);
            Ok(Box::new(WaterfallApp::new(autoconnect)))
        }),
    )
}

fn log_native_sdr_availability() {
    #[cfg(feature = "airspy")]
    if !hfsdr::native_sdr::airspy_available() {
        log::warn(
            "Airspy HF+ disabled: libairspyhf not found (bundled next to hfsdr or via system package; KiwiSDR and QMX still work)",
        );
    }
    #[cfg(feature = "rtlsdr")]
    if !hfsdr::native_sdr::rtlsdr_available() {
        log::warn(
            "RTL-SDR disabled: librtlsdr not found (bundled next to hfsdr or via system package; KiwiSDR and QMX still work)",
        );
    }
    #[cfg(feature = "soapy")]
    if !hfsdr::native_sdr::soapy_available() {
        log::warn(
            "SoapySDR disabled: libSoapySDR not found (bundle next to hfsdr or install system package)",
        );
    }
}
