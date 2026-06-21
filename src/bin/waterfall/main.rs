//! Live waterfall + spectrum UI over any [`IqSource`], on egui's wgpu backend.
//!
//! Usage:
//!   waterfall airspy [sample_rate_hz] [center_hz]
//!   waterfall kiwi <host> [port] [center_hz]

mod app;
mod audio;
mod colormap;
mod controls;
mod display_levels;
mod engine;
mod interaction;
mod settings;
mod skimmer;
mod smooth;
mod source;
mod theme;
mod widgets;

use app::WaterfallApp;
use eframe::egui;

fn main() -> eframe::Result {
    // The source is no longer built here: the GUI opens immediately and the
    // engine thread connects (auto-connecting if CLI args were supplied), so a
    // missing or slow front end never blocks or crashes the app.
    let autoconnect = source::request_from_args();

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
        Box::new(move |_cc| Ok(Box::new(WaterfallApp::new(autoconnect)))),
    )
}
