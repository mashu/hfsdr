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

    /// Allocating form of [`Self::waterfall_row_db_into`], for tests and callers
    /// that are not on the per-frame path.
    #[cfg(test)]
    pub(crate) fn waterfall_row_db_for_viewport(
        &self,
        row_index: usize,
        view: &hfsdr::SpectrumViewMapping,
        width: usize,
        avg: usize,
    ) -> Vec<f32> {
        let mut scratch = RowComposeScratch::default();
        self.waterfall_row_db_into(row_index, view, width, avg, &mut scratch);
        scratch.acc
    }
}
