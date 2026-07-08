//! Plot interaction state, row history, and waterfall texture cache.

use std::collections::VecDeque;
use std::time::Instant;

use eframe::egui;

use crate::interaction::{PlotInteraction, PlotViewState};
use crate::widgets::{PanadapterPlot, TraceViewKey};

use super::plot_cache::WaterfallTextureCache;

pub struct PlotState {
    pub rows: VecDeque<Vec<f32>>,
    pub latest: Vec<f32>,
    pub smoothed_trace: Vec<f32>,
    pub trace_composed: Vec<f32>,
    pub trace_view_key: TraceViewKey,
    pub overview_smoothed: Vec<f32>,
    pub overview_composed: Vec<f32>,
    pub overview_view_key: TraceViewKey,
    pub latest_frame_tick: bool,
    pub waterfall: WaterfallTextureCache,
    pub last_display_levels_at: Option<Instant>,
    pub panadapter_plot: PanadapterPlot,
    pub plot_view: PlotViewState,
    pub plot_interaction: PlotInteraction,
    pub hover_offset_hz: Option<f64>,
    pub last_plot_interaction_rect: Option<egui::Rect>,
    pub filter_overlay: FilterOverlayCache,
    pub tune_preview_offset_hz: Option<f64>,
    /// Last click-to-tune retune — clicks are ignored until the display has
    /// had time to show the new center (stale-row race protection).
    pub last_center_tune: Option<Instant>,
}

/// How long after a click-to-tune the plot rows may still show the old
/// center. Clicks inside this window would tune by a stale offset.
const CENTER_TUNE_SETTLE: std::time::Duration = std::time::Duration::from_millis(350);

impl PlotState {
    pub fn mark_center_tune(&mut self) {
        self.last_center_tune = Some(Instant::now());
    }

    pub fn center_tune_settled(&self) -> bool {
        self.last_center_tune
            .is_none_or(|t| t.elapsed() >= CENTER_TUNE_SETTLE)
    }
}

#[derive(Clone, Debug, Default)]
pub struct FilterOverlayCache {
    pub overlay: hfsdr::FilterOverlay,
    pub key: u64,
}
