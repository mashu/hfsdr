use crate::app::WaterfallApp;
use crate::app::prelude::*;

impl WaterfallApp {
    pub(crate) fn set_waterfall_gpu_available(&mut self, available: bool) {
        self.plot.waterfall.gpu_available = available;
    }

    /// True when the shader path should paint this frame.
    pub(crate) fn waterfall_gpu_active(&self) -> bool {
        self.plot.waterfall.gpu_available && self.display.waterfall_gpu
    }

    /// Frequency span covered by the rows uploaded to the ring texture.
    ///
    /// Not the same as the CPU path's storage span. The CPU path *composes*
    /// each row into a padded storage buffer, so its rows really do cover
    /// `view_span_hz`; the shader path uploads rows raw, so they cover only the
    /// rate they were transformed at. Using the padded span here stretches a
    /// 12 kHz Kiwi row across the whole band-overview width, which puts every
    /// carrier at the wrong frequency.
    pub(crate) fn waterfall_gpu_row_span_hz(&self) -> f32 {
        self.waterfall_storage_view().row_rate_hz
    }

    /// Build this frame's shader payload, or `None` when the CPU path is active.
    ///
    /// The visible window becomes texture coordinates via `offset_hz_to_storage_u`,
    /// so pan and zoom are two floats rather than a 360-row recompose.
    pub(crate) fn build_waterfall_gpu_callback(
        &mut self,
        plot_px: f32,
    ) -> Option<crate::widgets::WaterfallCallback> {
        if !self.waterfall_gpu_active() {
            return None;
        }
        let width = self.plot.waterfall.gpu_row_width;
        if width == 0 {
            return None;
        }
        let row_span_hz = self.waterfall_gpu_row_span_hz();
        let view = self.spectrum_view();
        let half = f64::from(view.view_span_hz) / 2.0;
        let u0 = hfsdr::offset_hz_to_storage_u(view.pan_offset_hz - half, row_span_hz);
        let u1 = hfsdr::offset_hz_to_storage_u(view.pan_offset_hz + half, row_span_hz);
        let uniforms = crate::widgets::waterfall_gpu_uniforms(
            u0,
            u1,
            self.plot.waterfall.gpu_row_head,
            width,
            plot_px,
            self.display.ref_db,
            self.display.range_db,
        );
        Some(crate::widgets::WaterfallCallback {
            uniforms,
            new_rows: std::mem::take(&mut self.plot.waterfall.gpu_pending),
            row_width: width,
        })
    }

    /// Queue `count` newly arrived dB rows for upload to the ring texture.
    ///
    /// Rows go up raw and full-span: the shader does the view window, so pan and
    /// zoom never touch this path.
    pub(crate) fn queue_waterfall_gpu_rows(&mut self, count: usize) {
        if !self.waterfall_gpu_active() || count == 0 {
            return;
        }
        let width = self.plot.rows.front().map(|r| r.len()).unwrap_or(0);
        if width == 0 {
            return;
        }
        if self.plot.waterfall.gpu_row_width != width {
            // Width change reallocates the texture; the old ring is stale.
            self.plot.waterfall.gpu_row_width = width;
            self.plot.waterfall.gpu_pending.clear();
            self.plot.waterfall.gpu_row_head = 0;
        }
        // `plot.rows` is newest-first; upload oldest of the new batch first so
        // ring order matches arrival order.
        let take = count.min(self.plot.rows.len());
        for i in (0..take).rev() {
            let Some(row) = self.plot.rows.get(i) else { continue };
            if row.len() != width {
                continue;
            }
            let slot = self.plot.waterfall.gpu_row_head;
            self.plot.waterfall.gpu_pending.push((slot, row.clone()));
            self.plot.waterfall.gpu_row_head = (slot + 1) % WATERFALL_ROWS;
        }
    }

    /// Index into `plot.rows` for the FFT row aligned with the waterfall top line.
    pub(crate) fn waterfall_trace_row_index(&self) -> usize {
        self.plot
            .waterfall
            .pending_viewport_row_appends
            .min(self.plot.rows.len().saturating_sub(1))
    }

    pub(crate) fn waterfall_source_row(&self, row_index: usize) -> Option<&[f32]> {
        if let Some(row) = self.plot.rows.get(row_index) {
            return Some(row.as_slice());
        }
        (row_index == 0 && !self.plot.latest.is_empty()).then_some(self.plot.latest.as_slice())
    }

    /// Compose one averaged waterfall row into `scratch.acc`, allocation-free.
    ///
    /// Runs once per row per frame times the averaging depth, so every buffer
    /// here is caller-owned and reused rather than freshly allocated.
    pub(crate) fn waterfall_row_db_into(
        &self,
        row_index: usize,
        view: &hfsdr::SpectrumViewMapping,
        width: usize,
        avg: usize,
        scratch: &mut RowComposeScratch,
    ) {
        let width = width.max(1);
        scratch.acc.clear();
        scratch.acc.resize(width, 0.0);
        let mut count = 0usize;
        for k in 0..avg {
            let Some(row_data) = self.waterfall_source_row(row_index.saturating_add(k)) else {
                break;
            };
            compose_panadapter_row_into(
                row_data,
                view.row_rate_hz,
                view.view_span_hz,
                view.data_span_hz,
                view.compose_pan_offset_hz,
                view.allow_band_padding,
                &mut scratch.composed,
                &mut scratch.compose_work,
            );
            stretch_row_to_width_into(&scratch.composed, width, &mut scratch.stretched);
            for (a, &v) in scratch
                .acc
                .iter_mut()
                .zip(scratch.stretched.iter().take(width))
            {
                *a += v;
            }
            count += 1;
        }
        if count == 0 {
            scratch.acc.clear();
            scratch.acc.resize(width, -120.0);
            return;
        }
        let inv = 1.0 / count as f32;
        for v in &mut scratch.acc {
            *v *= inv;
        }
    }

}
