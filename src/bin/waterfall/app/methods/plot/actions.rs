use crate::app::WaterfallApp;
use crate::app::prelude::*;

impl WaterfallApp {

    pub(crate) fn apply_plot_actions(&mut self, actions: Vec<PlotAction>) {
        let iq_playback = self.stats.iq_playback;
        for action in actions {
            match action {
                PlotAction::TuneDeltaHz(delta) => {
                    if iq_playback {
                        self.plot.plot_view.pan_offset_hz += delta;
                        self.plot.plot_view.clamp_pan(
                            self.plot_full_span_hz(),
                            self.plot_max_zoom_out(),
                        );
                    } else {
                        self.invalidate_waterfall_history();
                        self.radio.center_khz += delta / 1000.0;
                    }
                }
                PlotAction::CenterOnOffsetHz(offset) => {
                    if iq_playback {
                        self.radio.rit_hz = (offset as f32).clamp(RIT_MIN_HZ, RIT_MAX_HZ);
                        self.plot.tune_preview_offset_hz = None;
                    } else {
                        self.invalidate_waterfall_history();
                        self.radio.center_khz += offset / 1000.0;
                        self.plot.plot_view.pan_offset_hz = 0.0;
                        self.plot.tune_preview_offset_hz = None;
                        self.clear_rit();
                    }
                }
                PlotAction::SetTunePreviewOffsetHz(offset) => {
                    self.plot.tune_preview_offset_hz = Some(offset);
                }
                PlotAction::CommitTunePreview => {
                    if let Some(offset) = self.plot.tune_preview_offset_hz {
                        if iq_playback {
                            self.radio.rit_hz = (self.radio.rit_hz as f64 + offset)
                                .clamp(RIT_MIN_HZ as f64, RIT_MAX_HZ as f64)
                                as f32;
                        } else {
                            self.invalidate_waterfall_history();
                            self.radio.center_khz += offset / 1000.0;
                            self.plot.plot_view.pan_offset_hz = 0.0;
                            self.clear_rit();
                        }
                    }
                    self.plot.tune_preview_offset_hz = None;
                }
                PlotAction::ClearTunePreview => {
                    self.plot.tune_preview_offset_hz = None;
                }
                PlotAction::PanViewDeltaHz(delta) => {
                    self.plot.plot_view.pan_offset_hz += delta;
                    self.plot.plot_view.clamp_pan(
                        self.plot_full_span_hz(),
                        self.plot_max_zoom_out(),
                    );
                }
                PlotAction::ZoomView(factor) => {
                    self.plot.plot_view.zoom_by(
                        factor,
                        self.plot_full_span_hz(),
                        self.plot_max_zoom_out(),
                    );
                }
                PlotAction::SetPassbandHz(bw) => {
                    self.radio.cw.passband_hz =
                        bw.clamp(CW_PASSBAND_MIN_HZ, self.passband_max_hz());
                }
                PlotAction::SetRitHz(rit) => {
                    self.radio.rit_hz = rit.clamp(RIT_MIN_HZ, RIT_MAX_HZ);
                }
                PlotAction::SetViewPanHz(pan) => {
                    self.plot.plot_view.pan_offset_hz = pan;
                    self.plot.plot_view.clamp_pan(
                        self.plot_full_span_hz(),
                        self.plot_max_zoom_out(),
                    );
                }
                PlotAction::SetNotchOffset { slot, offset_hz } => {
                    if let Some(n) = self.radio.cw.notches.get_mut(slot) {
                        n.offset_hz = offset_hz;
                    }
                }
                PlotAction::SetNotchWidth { slot, width_hz } => {
                    if let Some(n) = self.radio.cw.notches.get_mut(slot) {
                        n.width_hz = width_hz.clamp(NOTCH_WIDTH_MIN_HZ, NOTCH_WIDTH_MAX_HZ);
                    }
                }
            }
        }
        self.clamp_center_to_ham_bands();
    }





    pub(crate) fn iq_passband_hz(&self) -> f32 {
        rf_view::iq_passband_hz(
            self.radio.is_kiwi,
            self.stats.iq_passband_hz,
            self.radio.sample_rate,
        )
    }



    /// Span of the spectrum FFT chain — base for zoom, pan, clicks, and waterfall storage.


    pub(crate) fn plot_full_span_hz(&self) -> f32 {
        rf_view::spectrum_plot_span_hz(self.stats.spectrum_rate, self.iq_passband_hz())
    }





    pub(crate) fn plot_max_zoom_out(&self) -> f32 {
        rf_view::max_zoom_out(
            self.radio.is_kiwi,
            self.iq_passband_hz(),
            self.band_overview_span_hz(),
        )
    }





    pub(crate) fn spectrum_view(&self) -> SpectrumViewMapping {
        rf_view::build_spectrum_view(
            self.radio.is_kiwi,
            self.iq_passband_hz(),
            self.plot_full_span_hz(),
            self.band_overview_span_hz(),
            self.stats.spectrum_rate,
            self.stats.spectrum_zoomed,
            &self.plot.plot_view,
        )
    }





    pub(crate) fn waterfall_storage_view(&self) -> SpectrumViewMapping {
        rf_view::build_waterfall_storage_view(
            self.radio.is_kiwi,
            self.iq_passband_hz(),
            self.plot_full_span_hz(),
            self.band_overview_span_hz(),
            self.stats.spectrum_rate,
        )
    }





    pub(crate) fn storage_row_width(&self, storage: &SpectrumViewMapping, row_len: usize) -> usize {
        panadapter_output_bins(row_len, storage.view_span_hz, storage.data_span_hz).max(1)
    }





    pub(crate) fn update_plot_hover(&mut self, ctx: &egui::Context) {
        let Some(rect) = self.plot.last_plot_interaction_rect else {
            self.plot.hover_offset_hz = None;
            return;
        };
        let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) else {
            self.plot.hover_offset_hz = None;
            return;
        };
        if !rect.contains(pos) {
            self.plot.hover_offset_hz = None;
            return;
        }
        self.plot.plot_view
            .clamp_pan(self.plot_full_span_hz(), self.plot_max_zoom_out());
        let view = self.spectrum_view();
        self.plot.hover_offset_hz = Some(crate::interaction::x_to_offset_hz(
            pos.x,
            rect,
            view.view_span_hz,
            view.pan_offset_hz,
        ));
    }




}
