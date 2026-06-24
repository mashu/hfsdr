// `tuning` — `WaterfallApp` methods.

    /// Keep RX center inside amateur band allocations when band lock is enabled.
    fn clamp_center_to_ham_bands(&mut self) {
        if !self.lock_ham_bands {
            return;
        }
        let clamped_khz = ham_bands::clamp_hz(self.center_khz * 1000.0) / 1000.0;
        if (clamped_khz - self.center_khz).abs() > f64::EPSILON {
            self.invalidate_waterfall_history();
            self.center_khz = clamped_khz;
        }
    }



    /// Snap tuning so the strongest signal near the cursor lands at the BFO pitch.
    fn clear_rit(&mut self) {
        self.rit_hz = 0.0;
        if self.pitch_lock {
            self.pitch_lock = false;
        }
    }



    /// Snap carrier to the strongest signal in view and clear listen offset.
    fn zero_beat(&mut self) {
        let listen = self.listen_offset_hz() as f32;
        let view = self.spectrum_view();
        if let Some(peak) = strongest_offset_hz(&self.latest, view.row_rate_hz, listen, 400.0) {
            self.center_khz += (peak - listen) as f64 / 1000.0;
            self.clamp_center_to_ham_bands();
            self.invalidate_waterfall_history();
            self.clear_rit();
            self.tune_preview_offset_hz = None;
        }
    }



    /// Continuously steer RIT so a drifting signal keeps a constant audio pitch.
    fn apply_pitch_lock(&mut self) {
        if !self.pitch_lock {
            return;
        }
        let listen = self.listen_offset_hz() as f32;
        let view = self.spectrum_view();
        if let Some(peak) = strongest_offset_hz(&self.latest, view.row_rate_hz, listen, 250.0) {
            let preview = self.tune_preview_offset_hz.unwrap_or(0.0) as f32;
            let target = (peak - preview).clamp(-800.0, 800.0);
            self.rit_hz = 0.85 * self.rit_hz + 0.15 * target;
        }
    }



    fn listen_offset_hz(&self) -> f64 {
        self.rit_hz as f64 + self.tune_preview_offset_hz.unwrap_or(0.0)
    }



    fn center_hz(&self) -> f64 {
        self.center_khz * 1000.0
    }



    fn cw_band_for_center(center_hz: f64) -> Option<&'static CwBandPreset> {
        CW_HF_BAND_PRESETS
            .iter()
            .chain(CW_VHF_BAND_PRESETS.iter())
            .find(|band| (center_hz - band.center_hz).abs() < 25_000.0)
    }



    fn band_preset_buttons(&mut self, ui: &mut egui::Ui, bands: &[CwBandPreset]) {
        ui.horizontal_wrapped(|ui| {
            for band in bands {
                let selected = (self.center_khz * 1000.0).round() == band.center_hz;
                if ui.selectable_label(selected, band.label).clicked() {
                    self.select_cw_band(band);
                }
            }
        });
    }



    fn band_overview_span_hz(&self) -> f32 {
        let iq = self.plot_full_span_hz();
        let center = self.center_khz * 1000.0;
        Self::cw_band_for_center(center)
            .map(|band| band.segment_hz.max(iq))
            .unwrap_or(iq)
    }



    /// Default panadapter span: CW segment for the current band (wider than IQ on Kiwi).
    fn default_cw_segment_hz(&self) -> f32 {
        let center = self.center_khz * 1000.0;
        Self::cw_band_for_center(center)
            .map(|band| band.segment_hz)
            .unwrap_or(self.band_overview_span_hz())
    }



    fn apply_default_view_zoom(&mut self) {
        self.plot_view.zoom_to_full_span();
        self.plot_view
            .clamp_pan(self.plot_full_span_hz(), self.plot_max_zoom_out());
    }



    fn select_cw_band(&mut self, band: &CwBandPreset) {
        self.center_khz = band.center_hz / 1000.0;
        self.plot_view.pan_offset_hz = 0.0;
        self.tune_preview_offset_hz = None;
        self.clear_rit();
        self.invalidate_waterfall_history();
        self.apply_radio_settings();
    }



    fn tune_to_hz(&mut self, frequency_hz: f64) {
        if (frequency_hz / 1000.0 - self.center_khz).abs() > f64::EPSILON {
            self.invalidate_waterfall_history();
        }
        self.center_khz = frequency_hz / 1000.0;
        self.clamp_center_to_ham_bands();
        self.plot_view.pan_offset_hz = 0.0;
        self.tune_preview_offset_hz = None;
        self.clear_rit();
    }

