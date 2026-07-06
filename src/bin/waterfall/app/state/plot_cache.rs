//! Waterfall GPU/CPU texture cache (storage + viewport).

use std::time::Instant;

use eframe::egui::{self, Color32};

use crate::app::{StorageKey, ViewportKey};
use crate::waterfall_perf::WaterfallPerf;

pub struct WaterfallTextureCache {
    pub storage_pixels: Vec<Color32>,
    pub storage_tex_width: usize,
    pub last_storage_key: Option<StorageKey>,
    pub viewport_texture: Option<egui::TextureHandle>,
    pub viewport_pixels: Vec<Color32>,
    pub viewport_tex_width: usize,
    /// Next ring-buffer row to write (0..WATERFALL_ROWS).
    pub viewport_row_head: usize,
    pub last_viewport_key: Option<ViewportKey>,
    pub textures_dirty: bool,
    pub force_texture_full: bool,
    pub pending_row_appends: usize,
    pub pending_viewport_row_appends: usize,
    /// Fractional row credit for time-paced scroll (see `waterfall_scroll_rows_due`).
    pub scroll_pacing_credit: f32,
    pub scroll_pacing_last: Option<Instant>,
    /// egui pass id — at most one paced apply per frame.
    pub scroll_pacing_pass: u64,
    /// Set when new waterfall rows were painted — trace must follow the displayed row, not `latest`.
    pub trace_refresh: bool,
    pub perf: WaterfallPerf,
}

impl Default for WaterfallTextureCache {
    fn default() -> Self {
        Self {
            storage_pixels: Vec::new(),
            storage_tex_width: 0,
            last_storage_key: None,
            viewport_texture: None,
            viewport_pixels: Vec::new(),
            viewport_tex_width: 0,
            viewport_row_head: 0,
            last_viewport_key: None,
            textures_dirty: true,
            force_texture_full: false,
            pending_row_appends: 0,
            pending_viewport_row_appends: 0,
            scroll_pacing_credit: 0.0,
            scroll_pacing_last: None,
            scroll_pacing_pass: u64::MAX,
            trace_refresh: false,
            perf: WaterfallPerf::default(),
        }
    }
}
