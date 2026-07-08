use crate::app::WaterfallApp;
use crate::app::prelude::*;

impl WaterfallApp {

    pub(crate) fn apply_plot_actions(&mut self, actions: Vec<PlotAction>) {
        let iq_playback = self.engine_ui.stats.iq_playback;
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
                    // Right after a click-to-tune the plot still shows rows
                    // from the old center until the retuned stream arrives; a
                    // second click in that window (double-click, impatient
                    // re-click) would tune by a stale offset all over again.
                    if !self.plot.center_tune_settled() {
                        continue;
                    }
                    if iq_playback {
                        self.radio.rit_hz = (offset as f32).clamp(RIT_MIN_HZ, RIT_MAX_HZ);
                        self.radio.rit_on = true;
                        self.plot.tune_preview_offset_hz = None;
                        self.sync_filter_to_listen();
                    } else {
                        self.invalidate_waterfall_history();
                        self.reset_trace_after_retune();
                        self.radio.center_khz += offset / 1000.0;
                        self.plot.plot_view.pan_offset_hz = 0.0;
                        self.plot.tune_preview_offset_hz = None;
                        self.clear_rit();
                        self.sync_filter_to_listen();
                        self.plot.mark_center_tune();
                    }
                }
                PlotAction::PanViewDeltaHz(delta) => {
                    self.plot.plot_view.pan_offset_hz += delta;
                    self.plot.plot_view.clamp_pan(
                        self.plot_full_span_hz(),
                        self.plot_max_zoom_out(),
                    );
                }
                PlotAction::ZoomView { factor, anchor_offset_hz } => {
                    let full_span = self.plot_full_span_hz();
                    let max_zoom = self.plot_max_zoom_out();
                    let span_before =
                        self.plot.plot_view.view_span_hz(full_span, max_zoom) as f64;
                    self.plot.plot_view.zoom_by(factor, full_span, max_zoom);
                    if let Some(anchor) = anchor_offset_hz {
                        // Keep the frequency under the cursor fixed on screen:
                        // scale its distance from the view centre by the span
                        // ratio, then re-clamp.
                        let span_after =
                            self.plot.plot_view.view_span_hz(full_span, max_zoom) as f64;
                        if span_before > 0.0 {
                            let ratio = span_after / span_before;
                            let pan = self.plot.plot_view.pan_offset_hz;
                            self.plot.plot_view.pan_offset_hz =
                                anchor - (anchor - pan) * ratio;
                            self.plot.plot_view.clamp_pan(full_span, max_zoom);
                        }
                    }
                }
                PlotAction::SetPassbandHz(bw) => {
                    self.radio.cw.passband_hz =
                        bw.clamp(CW_PASSBAND_MIN_HZ, self.passband_max_hz());
                }
                PlotAction::SetFilterShiftHz(shift) => {
                    self.radio.cw.filter_shift_hz = shift;
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
            self.engine_ui.stats.iq_passband_hz,
            self.radio.sample_rate,
        )
    }



    /// Span of the spectrum FFT chain — base for zoom, pan, clicks, and waterfall storage.
    pub(crate) fn plot_full_span_hz(&self) -> f32 {
        rf_view::spectrum_plot_span_hz(self.engine_ui.stats.spectrum_rate, self.iq_passband_hz())
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
            self.engine_ui.stats.spectrum_rate,
            self.engine_ui.stats.spectrum_zoomed,
            &self.plot.plot_view,
        )
    }





    pub(crate) fn waterfall_storage_view(&self) -> SpectrumViewMapping {
        rf_view::build_waterfall_storage_view(
            self.radio.is_kiwi,
            self.iq_passband_hz(),
            self.plot_full_span_hz(),
            self.band_overview_span_hz(),
            self.engine_ui.stats.spectrum_rate,
        )
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
