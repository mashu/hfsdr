use crate::app::WaterfallApp;
use crate::app::prelude::*;

impl WaterfallApp {

    /// Keep RX center inside amateur band allocations when band lock is enabled.
    pub(crate) fn clamp_center_to_ham_bands(&mut self) {
        if !self.radio.lock_ham_bands {
            return;
        }
        let clamped_khz = ham_bands::clamp_hz(self.radio.center_khz * 1000.0) / 1000.0;
        if (clamped_khz - self.radio.center_khz).abs() > f64::EPSILON {
            self.invalidate_waterfall_history();
            self.radio.center_khz = clamped_khz;
        }
    }



    /// Snap tuning so the strongest signal near the cursor lands at the BFO pitch.
    pub(crate) fn clear_rit(&mut self) {
        self.radio.rit_hz = 0.0;
        if self.radio.pitch_lock {
            self.radio.pitch_lock = false;
        }
    }



    /// Snap carrier to the strongest signal in view and clear listen offset.
    pub(crate) fn zero_beat(&mut self) {
        let listen = self.listen_offset_hz() as f32;
        let view = self.spectrum_view();
        if let Some(peak) = strongest_offset_hz(&self.plot.latest, view.row_rate_hz, listen, 400.0) {
            self.radio.center_khz += (peak - listen) as f64 / 1000.0;
            self.clamp_center_to_ham_bands();
            self.invalidate_waterfall_history();
            self.clear_rit();
            self.plot.tune_preview_offset_hz = None;
            self.sync_filter_to_listen();
        }
    }



    /// Continuously steer RIT so a drifting signal keeps a constant audio pitch.
    pub(crate) fn apply_pitch_lock(&mut self) {
        if !self.radio.pitch_lock {
            return;
        }
        let listen = self.listen_offset_hz() as f32;
        let view = self.spectrum_view();
        if let Some(peak) = strongest_offset_hz(&self.plot.latest, view.row_rate_hz, listen, 250.0) {
            let preview = self.plot.tune_preview_offset_hz.unwrap_or(0.0) as f32;
            let target = (peak - preview).clamp(-800.0, 800.0);
            self.radio.rit_hz = 0.85 * self.radio.rit_hz + 0.15 * target;
        }
    }



    pub(crate) fn listen_offset_hz(&self) -> f64 {
        self.radio.rit_hz as f64 + self.plot.tune_preview_offset_hz.unwrap_or(0.0)
    }



    /// Re-center the bandpass on the VFO after an explicit tune (click / center drag).
    pub(crate) fn sync_filter_to_listen(&mut self) {
        self.radio.cw.filter_shift_hz = ChannelOffsetHz::ZERO;
    }



    pub(crate) fn center_hz(&self) -> f64 {
        self.radio.center_khz * 1000.0
    }



    pub(crate) fn cw_band_for_center(center_hz: f64) -> Option<&'static CwBandPreset> {
        CW_HF_BAND_PRESETS
            .iter()
            .chain(CW_VHF_BAND_PRESETS.iter())
            .find(|band| (center_hz - band.center_hz).abs() < 25_000.0)
    }



    pub(crate) fn band_preset_buttons(&mut self, ui: &mut egui::Ui, bands: &[CwBandPreset]) {
        ui.horizontal_wrapped(|ui| {
            for band in bands {
                let selected = (self.radio.center_khz * 1000.0).round() == band.center_hz;
                if ui.selectable_label(selected, band.label).clicked() {
                    self.select_cw_band(band);
                }
            }
        });
    }



    pub(crate) fn band_overview_span_hz(&self) -> f32 {
        let iq = self.plot_full_span_hz();
        let center = self.radio.center_khz * 1000.0;
        Self::cw_band_for_center(center)
            .map(|band| band.segment_hz.max(iq))
            .unwrap_or(iq)
    }



    /// Default panadapter span: CW segment for the current band (wider than IQ on Kiwi).
    pub(crate) fn default_cw_segment_hz(&self) -> f32 {
        let center = self.radio.center_khz * 1000.0;
        Self::cw_band_for_center(center)
            .map(|band| band.segment_hz)
            .unwrap_or(self.band_overview_span_hz())
    }



    pub(crate) fn apply_default_view_zoom(&mut self) {
        self.plot.plot_view.zoom_to_full_span();
        self.plot.plot_view
            .clamp_pan(self.plot_full_span_hz(), self.plot_max_zoom_out());
    }



    pub(crate) fn select_cw_band(&mut self, band: &CwBandPreset) {
        self.radio.center_khz = band.center_hz / 1000.0;
        self.plot.plot_view.pan_offset_hz = 0.0;
        self.plot.tune_preview_offset_hz = None;
        self.clear_rit();
        self.sync_filter_to_listen();
        self.invalidate_waterfall_history();
        self.apply_radio_settings();
    }



    pub(crate) fn tune_to_hz(&mut self, frequency_hz: f64) {
        if (frequency_hz / 1000.0 - self.radio.center_khz).abs() > f64::EPSILON {
            self.invalidate_waterfall_history();
        }
        self.radio.center_khz = frequency_hz / 1000.0;
        self.clamp_center_to_ham_bands();
        self.plot.plot_view.pan_offset_hz = 0.0;
        self.plot.tune_preview_offset_hz = None;
        self.clear_rit();
        self.sync_filter_to_listen();
    }


}
