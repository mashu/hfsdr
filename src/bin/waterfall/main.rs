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
mod filter_curve_plot;
mod filter_design_panel;
mod envelope_diagnostic;
mod pipeline_flow;
mod log;
mod popup;
mod rf_view;
mod settings;
mod source;
mod status_icons;
mod status_widgets;
mod theme;
mod widgets;
#[cfg(not(feature = "gui-core"))]
mod web_demo;
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

#[cfg(feature = "gui-core")]
fn main() -> eframe::Result {
    log::init();
    hfsdr::dsp_pool::init();
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
            // GPU waterfall needs resources in egui's wgpu renderer; when the
            // backend is not wgpu this returns false and the CPU path is used.
            let gpu = crate::widgets::install_waterfall_gpu(cc);
            log::info(if gpu {
                "waterfall: GPU shader path"
            } else {
                "waterfall: CPU path (no wgpu render state)"
            });
            let mut app = WaterfallApp::new(autoconnect);
            app.set_waterfall_gpu_available(gpu);
            Ok(Box::new(app))
        }),
    )
}


/// Browser entry point.
///
/// The desktop `main` cannot be reused: it calls `run_native`, loads native SDR
/// drivers via dlopen, and starts an engine thread — none of which exist in a
/// tab. The UI itself is the same `WaterfallApp`, so every panel, the filter
/// chain, the VFO and the waterfall are the real ones.
#[cfg(all(target_arch = "wasm32", not(feature = "gui-core")))]
fn main() {
    use wasm_bindgen::JsCast as _;

    // Panics otherwise vanish into the console with no stack.
    std::panic::set_hook(Box::new(|info| {
        web_sys::console::error_1(&format!("hfsdr panic: {info}").into());
    }));

    wasm_bindgen_futures::spawn_local(async {
        let document = web_sys::window()
            .expect("no window")
            .document()
            .expect("no document");
        let canvas = document
            .get_element_by_id("hfsdr_canvas")
            .expect("missing #hfsdr_canvas")
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("#hfsdr_canvas is not a canvas");

        let result = eframe::WebRunner::new()
            .start(
                canvas,
                eframe::WebOptions::default(),
                Box::new(|cc| {
                    crate::theme::apply(&cc.egui_ctx);
                    let gpu = crate::widgets::install_waterfall_gpu(cc);
                    let mut app = WaterfallApp::new_for_web();
                    app.set_waterfall_gpu_available(gpu);
                    Ok(Box::new(app) as Box<dyn eframe::App>)
                }),
            )
            .await;

        // Replace the loading text either way, so a failure is visible.
        if let Some(el) = document.get_element_by_id("loading") {
            match result {
                Ok(_) => el.remove(),
                Err(e) => el.set_text_content(Some(&format!("Failed to start: {e:?}"))),
            }
        }
    });
}

/// Non-wasm builds without gui-core have no entry point of their own.
#[cfg(all(not(target_arch = "wasm32"), not(feature = "gui-core")))]
fn main() {
    eprintln!("hfsdr: this build has no frontend; enable gui-core or build for wasm32");
}

#[cfg(feature = "gui-core")]
fn log_native_sdr_availability() {
    #[cfg(feature = "airspy")]
    if !hfsdr::native_sdr::airspy_available() {
        log::warn(
            "Airspy HF+ disabled: libairspyhf not found (bundled next to hfsdr or via system package; KiwiSDR and QMX still work)",
        );
    } else {
        log::info("Airspy HF+: libairspyhf available");
    }
    #[cfg(feature = "rtlsdr")]
    if !hfsdr::native_sdr::rtlsdr_available() {
        log::warn(
            "RTL-SDR disabled: librtlsdr not found (bundled next to hfsdr or via system package; KiwiSDR and QMX still work)",
        );
    } else {
        log::info("RTL-SDR: librtlsdr available");
    }
    #[cfg(feature = "soapy")]
    if !hfsdr::native_sdr::soapy_available() {
        log::warn(
            "SoapySDR disabled: libSoapySDR not found (bundle next to hfsdr or install system package)",
        );
    }
    // Soapy driver/module details are logged from native_sdr::init → soapy::log_startup_status.
}
