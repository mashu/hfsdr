use crate::app::WaterfallApp;
use crate::app::prelude::*;

impl WaterfallApp {
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
        (row_index == 0 && !self.plot.latest.is_empty()).then(|| self.plot.latest.as_slice())
    }

    pub(crate) fn waterfall_row_db_for_viewport(
        &self,
        row_index: usize,
        view: &hfsdr::SpectrumViewMapping,
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
}
