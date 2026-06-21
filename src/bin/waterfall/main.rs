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
mod interaction;
mod smooth;
mod source;
mod theme;
mod widgets;

use app::WaterfallApp;
use eframe::egui;

fn main() -> eframe::Result {
    let (source, iq, sample_rate, center_hz, is_kiwi) = match source::build_source() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("could not start source: {e}");
            std::process::exit(1);
        }
    };

    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1580.0, 960.0])
            .with_min_inner_size([1420.0, 900.0])
            .with_title("hfsdr"),
        ..Default::default()
    };
    eframe::run_native(
        "hfsdr",
        options,
        Box::new(move |_cc| {
            Ok(Box::new(WaterfallApp::new(source, iq, sample_rate, center_hz, is_kiwi)))
        }),
    )
}
