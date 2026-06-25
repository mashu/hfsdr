use std::collections::VecDeque;
use std::time::Instant;

use eframe::egui::{self, Color32};

use crate::interaction::{PlotInteraction, PlotViewState};
use crate::widgets::{PanadapterPlot, TraceViewKey};

use crate::app::{StorageKey, ViewportKey};

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
    pub waterfall_storage_pixels: Vec<Color32>,
    pub storage_tex_width: usize,
    pub last_storage_key: Option<StorageKey>,
    pub waterfall_viewport_texture: Option<egui::TextureHandle>,
    pub waterfall_viewport_pixels: Vec<Color32>,
    pub viewport_tex_width: usize,
    pub last_viewport_key: Option<ViewportKey>,
    pub textures_dirty: bool,
    pub force_texture_full: bool,
    pub pending_row_appends: usize,
    pub pending_viewport_row_appends: usize,
    pub last_display_levels_at: Option<Instant>,
    pub waterfall_row_scratch: Vec<f32>,
    pub panadapter_plot: PanadapterPlot,
    pub plot_view: PlotViewState,
    pub plot_interaction: PlotInteraction,
    pub hover_offset_hz: Option<f64>,
    pub last_plot_interaction_rect: Option<egui::Rect>,
    pub tune_preview_offset_hz: Option<f64>,
}
