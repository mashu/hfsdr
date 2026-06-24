// `display/levels` — waterfall ref/range auto-tracking.

    fn lock_display_levels_for_rf_tuning(&mut self) {
        lock_display_levels_for_rf_tuning(
            &mut self.display_auto_track,
            &mut self.display_levels_initialized,
        );
    }

    fn update_display_levels(&mut self) {
        if !should_auto_adjust_display_levels(
            self.display_levels_initialized,
            self.display_auto_track,
        ) {
            return;
        }
        let target = self.estimate_display_levels();
        let Some(target) = target else {
            return;
        };
        let (ref_db, range_db) = if self.display_auto_track && self.display_levels_initialized {
            crate::display_levels::smooth_levels(
                (self.ref_db, self.range_db),
                target,
                0.06,
            )
        } else {
            target
        };
        let ref_delta = (self.ref_db - ref_db).abs();
        let range_delta = (self.range_db - range_db).abs();
        if !self.display_levels_initialized || ref_delta > 0.35 || range_delta > 0.75 {
            self.ref_db = ref_db;
            self.range_db = range_db;
            self.force_texture_full = true;
            self.textures_dirty = true;
            self.display_levels_initialized = true;
        }
    }

    fn estimate_display_levels(&self) -> Option<(f32, f32)> {
        const ROWS_FOR_ESTIMATE: usize = 24;
        let view = self.spectrum_view();
        let compose = |row: &[f32]| {
            compose_panadapter_row(
                row,
                view.row_rate_hz,
                view.view_span_hz,
                view.data_span_hz,
                view.compose_pan_offset_hz,
                view.allow_band_padding,
            )
        };
        if self.rows.len() >= 8 {
            let n = self.rows.len().min(ROWS_FOR_ESTIMATE);
            let composed: Vec<Vec<f32>> = self
                .rows
                .iter()
                .take(n)
                .map(|row| compose(row))
                .collect();
            let refs: Vec<&[f32]> = composed.iter().map(Vec::as_slice).collect();
            estimate_levels_from_rows(&refs).or_else(|| estimate_levels(&compose(&self.latest)))
        } else {
            estimate_levels(&compose(&self.latest))
        }
    }

    fn passband_max_hz(&self) -> f32 {
        if self.filter_wide {
            CW_PASSBAND_MAX_HZ
        } else {
            CW_PASSBAND_NARROW_MAX_HZ
        }
    }
