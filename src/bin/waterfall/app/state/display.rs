use std::time::Instant;

use eframe::egui;
use hfsdr::FftWindowKind;

#[derive(Clone, Debug)]
pub struct DisplayState {
    pub ref_db: f32,
    pub range_db: f32,
    pub display_levels_initialized: bool,
    pub display_auto_track: bool,
    pub show_band_overview: bool,
    pub pan_step_hz: f32,
    pub pan_step_fast_hz: f32,
    pub arrow_hold: Option<(egui::Key, Instant)>,
    pub smooth_alpha: f32,
    pub waterfall_avg: u8,
    /// FFT analysis window for panadapter / waterfall.
    pub spectrum_window: FftWindowKind,
    pub spectrum_kaiser_beta: f32,
    pub waterfall_rows: usize,
    pub target_fps: u32,
    pub fft_size: usize,
    pub fft_auto: bool,
    pub full_drain_spectrum: bool,
    /// Show per-pump DSP timings in the Performance panel (`HFSDR_PERF=1` also enables).
    pub perf_trace: bool,
}
