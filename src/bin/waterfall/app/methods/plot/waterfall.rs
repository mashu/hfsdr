use crate::app::WaterfallApp;
use crate::app::prelude::*;

impl WaterfallApp {

    pub(crate) fn waterfall_source_row(&self, row_index: usize) -> Option<&[f32]> {
        if let Some(row) = self.plot.rows.get(row_index) {
            return Some(row.as_slice());
        }
        // Only the newest row may fall back to the live FFT; older slots stay empty until
        // history refills so a tune reset cannot paint the whole waterfall as one column.
        (row_index == 0 && !self.plot.latest.is_empty()).then(|| self.plot.latest.as_slice())
    }





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





    pub(crate) fn waterfall_row_db_for_storage(
        &self,
        row_index: usize,
        storage: &SpectrumViewMapping,
        width: usize,
        avg: usize,
    ) -> Vec<f32> {
        let mut acc = vec![0.0f32; width];
        let mut count = 0usize;
        for k in 0..avg {
            let Some(row_data) = self.waterfall_source_row(row_index.saturating_add(k)) else {
                break;
            };
            let row = compose_panadapter_row(
                row_data,
                storage.row_rate_hz,
                storage.view_span_hz,
                storage.data_span_hz,
                storage.compose_pan_offset_hz,
                storage.allow_band_padding,
            );
            let n = row.len().min(width);
            for (i, &v) in row.iter().take(n).enumerate() {
                acc[i] += v;
            }
            count += 1;
        }
        if count == 0 {
            return vec![-120.0; width];
        }
        let inv = 1.0 / count as f32;
        for v in &mut acc {
            *v *= inv;
        }
        acc
    }





    pub(crate) fn waterfall_row_db_for_viewport(
        &self,
        row_index: usize,
        view: &SpectrumViewMapping,
        width: usize,
        avg: usize,
    ) -> Vec<f32> {
        let mut acc = vec![0.0f32; width.max(1)];
        let mut count = 0usize;
        for k in 0..avg {
            let Some(row_data) = self.waterfall_source_row(row_index.saturating_add(k)) else {
                break;
            };
            let row = compose_panadapter_row(
                row_data,
                view.row_rate_hz,
                view.view_span_hz,
                view.data_span_hz,
                view.compose_pan_offset_hz,
                view.allow_band_padding,
            );
            let stretched = stretch_row_to_width(&row, width);
            let n = stretched.len().min(width);
            for (i, &v) in stretched.iter().take(n).enumerate() {
                acc[i] += v;
            }
            count += 1;
        }
        if count == 0 {
            return vec![-120.0; width.max(1)];
        }
        let inv = 1.0 / count as f32;
        for v in &mut acc {
            *v *= inv;
        }
        acc
    }





    pub(crate) fn upload_waterfall_viewport(&mut self, ctx: &egui::Context, width: usize, height: usize) {
        let image = egui::ColorImage::new([width, height], self.plot.waterfall_viewport_pixels.clone());
        match &mut self.plot.waterfall_viewport_texture {
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
            .plot.rows
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
        let n_new = self.plot.pending_row_appends.min(h);
        let can_append = n_new > 0
            && n_new < h
            && !self.plot.force_texture_full
            && self.plot.last_storage_key == Some(key)
            && self.plot.storage_tex_width == w
            && self.plot.waterfall_storage_pixels.len() == w * h;

        if can_append {
            let stride = w;
            for y in (0..h - n_new).rev() {
                let src = y * stride;
                self.plot.waterfall_storage_pixels
                    .copy_within(src..src + stride, (y + n_new) * stride);
            }
            for y in 0..n_new {
                self.plot.waterfall_row_scratch =
                    self.waterfall_row_db_for_storage(y, &storage, w, avg);
                Self::write_row_pixels(
                    &mut self.plot.waterfall_storage_pixels,
                    y,
                    w,
                    &self.plot.waterfall_row_scratch,
                    ref_db,
                    range_db,
                );
            }
        } else if self.plot.textures_dirty
            || self.plot.force_texture_full
            || self.plot.last_storage_key != Some(key)
            || self.plot.storage_tex_width != w
            || self.plot.waterfall_storage_pixels.len() != w * h
        {
            self.plot.storage_tex_width = w;
            self.plot.waterfall_storage_pixels.resize(w * h, Color32::BLACK);
            for y in 0..h {
                let row_db = self.waterfall_row_db_for_storage(y, &storage, w, avg);
                Self::write_row_pixels(
                    &mut self.plot.waterfall_storage_pixels,
                    y,
                    w,
                    &row_db,
                    ref_db,
                    range_db,
                );
            }
            self.plot.last_storage_key = Some(key);
            self.plot.last_viewport_key = None;
        } else {
            return;
        }

        self.plot.textures_dirty = false;
        self.plot.pending_row_appends = 0;
        let _ = ctx; // storage is CPU-side; viewport upload happens in sync_waterfall_viewport
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

        let n_new = self.plot.pending_viewport_row_appends.min(h);
        let can_append = n_new > 0
            && n_new < h
            && !self.plot.force_texture_full
            && !self.plot.textures_dirty
            && self.plot.last_viewport_key == Some(key)
            && self.plot.waterfall_viewport_texture.is_some()
            && self.plot.viewport_tex_width == dst_w
            && self.plot.waterfall_viewport_pixels.len() == dst_w * h;

        if can_append {
            let stride = dst_w;
            for y in (0..h - n_new).rev() {
                let src = y * stride;
                self.plot.waterfall_viewport_pixels
                    .copy_within(src..src + stride, (y + n_new) * stride);
            }
            for y in 0..n_new {
                self.plot.waterfall_row_scratch =
                    self.waterfall_row_db_for_viewport(y, &view, dst_w, avg);
                Self::write_row_pixels(
                    &mut self.plot.waterfall_viewport_pixels,
                    y,
                    dst_w,
                    &self.plot.waterfall_row_scratch,
                    ref_db,
                    range_db,
                );
            }
            self.upload_waterfall_viewport(ctx, dst_w, h);
            self.plot.pending_viewport_row_appends = 0;
            return;
        }

        if self.plot.last_viewport_key == Some(key)
            && self.plot.waterfall_viewport_texture.is_some()
            && self.plot.viewport_tex_width == dst_w
            && self.plot.waterfall_viewport_pixels.len() == dst_w * h
            && !self.plot.textures_dirty
            && !self.plot.force_texture_full
        {
            self.plot.pending_viewport_row_appends = 0;
            return;
        }

        self.plot.waterfall_viewport_pixels.resize(dst_w * h, Color32::BLACK);
        for y in 0..h {
            let row_db = self.waterfall_row_db_for_viewport(y, &view, dst_w, avg);
            Self::write_row_pixels(
                &mut self.plot.waterfall_viewport_pixels,
                y,
                dst_w,
                &row_db,
                ref_db,
                range_db,
            );
        }
        self.plot.viewport_tex_width = dst_w;
        self.upload_waterfall_viewport(ctx, dst_w, h);
        self.plot.last_viewport_key = Some(key);
        self.plot.pending_viewport_row_appends = 0;
        self.plot.force_texture_full = false;
    }




}
