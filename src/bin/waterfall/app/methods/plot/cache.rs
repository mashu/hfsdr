//! Waterfall texture upload and incremental cache sync.

use std::time::Instant;

use eframe::egui::{self, Color32};

use crate::app::prelude::*;
use crate::app::WaterfallApp;
use crate::app::ViewportKey;
use crate::colormap::db_to_colour;

impl WaterfallApp {
    pub(crate) fn write_row_pixels(
        pixels: &mut [Color32],
        y: usize,
        width: usize,
        db_row: &[f32],
        ref_db: f32,
        range_db: f32,
    ) {
        let base = y * width;
        for (x, &db) in db_row.iter().enumerate().take(width) {
            pixels[base + x] = db_to_colour(db, ref_db, range_db);
        }
    }

    pub(crate) fn upload_waterfall_viewport_full(
        &mut self,
        ctx: &egui::Context,
        width: usize,
        height: usize,
    ) {
        let t0 = Instant::now();
        let cache = &mut self.plot.waterfall;
        let image = egui::ColorImage::new([width, height], cache.viewport_pixels.clone());
        match &mut cache.viewport_texture {
            Some(tex) => tex.set(image, egui::TextureOptions::NEAREST),
            none => {
                *none = Some(ctx.load_texture(
                    "waterfall_viewport",
                    image,
                    egui::TextureOptions::NEAREST,
                ));
            }
        }
        cache.perf.upload_ns = t0.elapsed().as_nanos() as u64;
        cache.perf.uploads_full += 1;
    }

    fn upload_waterfall_ring_rows(
        &mut self,
        ctx: &egui::Context,
        width: usize,
        head: usize,
        count: usize,
    ) {
        let count = count.min(WATERFALL_ROWS);
        if count == 0 || width == 0 {
            return;
        }
        let t0 = Instant::now();
        let h = WATERFALL_ROWS;
        let head = head % h;
        let stride = width;
        if self.plot.waterfall.viewport_pixels.len() < h * stride {
            return;
        }
        let needs_full = self.plot.waterfall.viewport_texture.is_none();
        if needs_full {
            self.upload_waterfall_viewport_full(ctx, width, h);
            return;
        }
        let pixels = &self.plot.waterfall.viewport_pixels;
        let upload_rows = |tex: &mut egui::TextureHandle, y: usize, rows: usize| {
            if rows == 0 {
                return;
            }
            let start = y * stride;
            let end = start + rows * stride;
            let patch = egui::ColorImage::new([width, rows], pixels[start..end].to_vec());
            tex.set_partial([0, y], patch, egui::TextureOptions::NEAREST);
        };
        if let Some(tex) = &mut self.plot.waterfall.viewport_texture {
            if head + count <= h {
                upload_rows(tex, head, count);
            } else {
                let first = h - head;
                upload_rows(tex, head, first);
                upload_rows(tex, 0, count - first);
            }
            self.plot.waterfall.perf.uploads_partial += 1;
        }
        self.plot.waterfall.perf.upload_ns = t0.elapsed().as_nanos() as u64;
    }

    pub(crate) fn sync_waterfall_viewport(&mut self, ctx: &egui::Context, plot_width: usize) {
        if self.plot.rows.is_empty() {
            return;
        }
        let view = self.spectrum_view();
        let dst_w = plot_width.max(1);
        let h = WATERFALL_ROWS;
        let key = ViewportKey::from_view(view.view_span_hz, view.pan_offset_hz, dst_w);
        let avg = self.display.waterfall_avg.max(1) as usize;
        let ref_db = self.display.ref_db;
        let range_db = self.display.range_db;
        let pending_start = self.plot.waterfall.pending_viewport_row_appends.min(h);
        let rows_per_frame = self.display.waterfall_rows_per_frame.max(1) as usize;
        let target_fps = self.effective_target_fps();
        let pass = ctx.cumulative_pass_nr();
        let n_apply = if pass != self.plot.waterfall.scroll_pacing_pass {
            self.plot.waterfall.scroll_pacing_pass = pass;
            let now = Instant::now();
            let dt = self
                .plot
                .waterfall
                .scroll_pacing_last
                .map(|t| now.duration_since(t).as_secs_f32())
                .unwrap_or(0.0);
            self.plot.waterfall.scroll_pacing_last = Some(now);
            if pending_start == 0 {
                self.plot.waterfall.scroll_pacing_credit = 0.0;
                0
            } else {
                let (n, credit) = waterfall_scroll_rows_due(
                    pending_start,
                    rows_per_frame,
                    target_fps,
                    dt,
                    self.plot.waterfall.scroll_pacing_credit,
                );
                self.plot.waterfall.scroll_pacing_credit = credit;
                n
            }
        } else {
            0
        };
        self.plot.waterfall.perf.rows_per_frame_cap = rows_per_frame as u32;
        self.plot.waterfall.perf.rows_pending = pending_start as u32;
        self.plot.waterfall.perf.sync_calls += 1;

        let can_append = n_apply > 0
            && !self.plot.waterfall.force_texture_full
            && !self.plot.waterfall.textures_dirty
            && self.plot.waterfall.last_viewport_key == Some(key)
            && self.plot.waterfall.viewport_texture.is_some()
            && self.plot.waterfall.viewport_tex_width == dst_w
            && self.plot.waterfall.viewport_pixels.len() == dst_w * h;

        if can_append {
            let t_compose = Instant::now();
            let head = self.plot.waterfall.viewport_row_head;
            for i in 0..n_apply {
                let y = (head + i) % h;
                let history_idx = pending_start - n_apply + i;
                let row_db =
                    self.waterfall_row_db_for_viewport(history_idx, &view, dst_w, avg);
                Self::write_row_pixels(
                    &mut self.plot.waterfall.viewport_pixels,
                    y,
                    dst_w,
                    &row_db,
                    ref_db,
                    range_db,
                );
            }
            self.plot.waterfall.perf.compose_ns = t_compose.elapsed().as_nanos() as u64;
            self.upload_waterfall_ring_rows(ctx, dst_w, head, n_apply);
            self.plot.waterfall.viewport_row_head = (head + n_apply) % h;
            self.plot.waterfall.pending_viewport_row_appends -= n_apply;
            self.plot.waterfall.perf.record_row_apply(n_apply as u32);
            self.plot.waterfall.trace_refresh = n_apply > 0;
            self.plot.waterfall.textures_dirty = false;
            return;
        }

        if pending_start == 0
            && self.plot.waterfall.last_viewport_key == Some(key)
            && self.plot.waterfall.viewport_texture.is_some()
            && self.plot.waterfall.viewport_tex_width == dst_w
            && self.plot.waterfall.viewport_pixels.len() == dst_w * h
            && !self.plot.waterfall.textures_dirty
            && !self.plot.waterfall.force_texture_full
        {
            return;
        }

        let t_compose = Instant::now();
        self.plot.waterfall.viewport_pixels.resize(dst_w * h, Color32::BLACK);
        let fill = h.min(self.plot.rows.len());
        let mut head = 0usize;
        for i in 0..fill {
            let history_idx = fill - 1 - i;
            let y = head;
            let row_db = self.waterfall_row_db_for_viewport(history_idx, &view, dst_w, avg);
            Self::write_row_pixels(
                &mut self.plot.waterfall.viewport_pixels,
                y,
                dst_w,
                &row_db,
                ref_db,
                range_db,
            );
            head = (head + 1) % h;
        }
        self.plot.waterfall.perf.compose_ns = t_compose.elapsed().as_nanos() as u64;
        self.plot.waterfall.viewport_tex_width = dst_w;
        self.plot.waterfall.viewport_row_head = head;
        self.upload_waterfall_viewport_full(ctx, dst_w, h);
        self.plot.waterfall.last_viewport_key = Some(key);
        self.plot.waterfall.pending_viewport_row_appends = 0;
        self.plot.waterfall.scroll_pacing_credit = 0.0;
        self.plot.waterfall.scroll_pacing_last = None;
        self.plot.waterfall.force_texture_full = false;
        self.plot.waterfall.textures_dirty = false;
        self.plot.waterfall.trace_refresh = fill > 0;
        self.plot.waterfall.perf.reset_interval();
    }
}
