//! Waterfall texture upload and incremental cache sync.

use eframe::egui::{self, Color32};

use hfsdr::{compose_panadapter_row, stretch_row_to_width};

use crate::app::prelude::*;
use crate::app::WaterfallApp;
use crate::app::{StorageKey, ViewportKey};
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

    pub(crate) fn upload_waterfall_viewport(
        &mut self,
        ctx: &egui::Context,
        width: usize,
        height: usize,
    ) {
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
    }

    pub(crate) fn sync_waterfall_storage(&mut self, ctx: &egui::Context) {
        if self.plot.rows.is_empty() {
            return;
        }
        let storage = self.waterfall_storage_view();
        let row_len = self
            .plot
            .rows
            .front()
            .map(|r| r.len())
            .unwrap_or_else(|| self.plot.latest.len());
        if row_len == 0 {
            return;
        }
        let w = self.storage_row_width(&storage, row_len);
        let h = WATERFALL_ROWS;
        let key = StorageKey::from_storage(&storage, w);
        let avg = self.display.waterfall_avg.max(1) as usize;
        let ref_db = self.display.ref_db;
        let range_db = self.display.range_db;
        let n_new = self.plot.waterfall.pending_row_appends.min(h);
        let can_append = n_new > 0
            && n_new < h
            && !self.plot.waterfall.force_texture_full
            && self.plot.waterfall.last_storage_key == Some(key)
            && self.plot.waterfall.storage_tex_width == w
            && self.plot.waterfall.storage_pixels.len() == w * h;

        if can_append {
            let stride = w;
            for y in (0..h - n_new).rev() {
                let src = y * stride;
                self.plot.waterfall.storage_pixels.copy_within(
                    src..src + stride,
                    (y + n_new) * stride,
                );
            }
            for y in 0..n_new {
                let row_db = self.waterfall_row_db_for_storage(y, &storage, w, avg);
                Self::write_row_pixels(
                    &mut self.plot.waterfall.storage_pixels,
                    y,
                    w,
                    &row_db,
                    ref_db,
                    range_db,
                );
            }
        } else if self.plot.waterfall.textures_dirty
            || self.plot.waterfall.force_texture_full
            || self.plot.waterfall.last_storage_key != Some(key)
            || self.plot.waterfall.storage_tex_width != w
            || self.plot.waterfall.storage_pixels.len() != w * h
        {
            self.plot.waterfall.storage_tex_width = w;
            self.plot.waterfall.storage_pixels.resize(w * h, Color32::BLACK);
            for y in 0..h {
                let row_db = self.waterfall_row_db_for_storage(y, &storage, w, avg);
                Self::write_row_pixels(
                    &mut self.plot.waterfall.storage_pixels,
                    y,
                    w,
                    &row_db,
                    ref_db,
                    range_db,
                );
            }
            self.plot.waterfall.last_storage_key = Some(key);
            self.plot.waterfall.last_viewport_key = None;
        } else {
            return;
        }

        self.plot.waterfall.textures_dirty = false;
        self.plot.waterfall.pending_row_appends = 0;
        let _ = ctx;
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
        let n_new = self.plot.waterfall.pending_viewport_row_appends.min(h);
        let can_append = n_new > 0
            && n_new < h
            && !self.plot.waterfall.force_texture_full
            && !self.plot.waterfall.textures_dirty
            && self.plot.waterfall.last_viewport_key == Some(key)
            && self.plot.waterfall.viewport_texture.is_some()
            && self.plot.waterfall.viewport_tex_width == dst_w
            && self.plot.waterfall.viewport_pixels.len() == dst_w * h;

        if can_append {
            let stride = dst_w;
            for y in (0..h - n_new).rev() {
                let src = y * stride;
                self.plot.waterfall.viewport_pixels.copy_within(
                    src..src + stride,
                    (y + n_new) * stride,
                );
            }
            for y in 0..n_new {
                let row_db = self.waterfall_row_db_for_viewport(y, &view, dst_w, avg);
                Self::write_row_pixels(
                    &mut self.plot.waterfall.viewport_pixels,
                    y,
                    dst_w,
                    &row_db,
                    ref_db,
                    range_db,
                );
            }
            self.upload_waterfall_viewport(ctx, dst_w, h);
            self.plot.waterfall.pending_viewport_row_appends = 0;
            return;
        }

        if self.plot.waterfall.last_viewport_key == Some(key)
            && self.plot.waterfall.viewport_texture.is_some()
            && self.plot.waterfall.viewport_tex_width == dst_w
            && self.plot.waterfall.viewport_pixels.len() == dst_w * h
            && !self.plot.waterfall.textures_dirty
            && !self.plot.waterfall.force_texture_full
        {
            self.plot.waterfall.pending_viewport_row_appends = 0;
            return;
        }

        self.plot.waterfall.viewport_pixels.resize(dst_w * h, Color32::BLACK);
        for y in 0..h {
            let row_db = self.waterfall_row_db_for_viewport(y, &view, dst_w, avg);
            Self::write_row_pixels(
                &mut self.plot.waterfall.viewport_pixels,
                y,
                dst_w,
                &row_db,
                ref_db,
                range_db,
            );
        }
        self.plot.waterfall.viewport_tex_width = dst_w;
        self.upload_waterfall_viewport(ctx, dst_w, h);
        self.plot.waterfall.last_viewport_key = Some(key);
        self.plot.waterfall.pending_viewport_row_appends = 0;
        self.plot.waterfall.force_texture_full = false;
    }
}
