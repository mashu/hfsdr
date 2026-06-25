use crate::app::WaterfallApp;
use crate::app::prelude::*;

impl WaterfallApp {

    pub(crate) fn display_section(&mut self, ui: &mut egui::Ui) {
        collapsible_section(
            ui,
            "display",
            "Display",
            Some(&[
                ("Navigation", ACCENT),
                (
                    "←/→ pans when zoomed, otherwise tunes RX. Hold to accelerate (2× then fast); Shift = fine, Ctrl = fast.",
                    MUTED,
                ),
                ("Minimap", ACCENT),
                (
                    "Top-right inset: CW band context + IQ data + viewport box. Click to pan (M).",
                    MUTED,
                ),
            ]),
            true,
            |ui| {
            let max_zoom = self.plot_max_zoom_out();
            scroll_slider_f32(ui, &mut self.plot.plot_view.zoom, 0.04..=max_zoom, "View zoom");
            self.plot.plot_view.clamp_pan(self.plot_full_span_hz(), max_zoom);
            let view = self.spectrum_view();
            let view_khz = view.view_span_hz / 1000.0;
            ui.label(
                egui::RichText::new(format!(
                    "Showing {view_khz:.1} kHz · zoom 1.0 = full IQ · {max_zoom:.1} = widest overview"
                ))
                .small()
                .color(MUTED),
            );
            ui.horizontal(|ui| {
                if ui.small_button("Full IQ (F)").clicked() {
                    self.plot.plot_view.zoom_to_full_span();
                }
                if self.radio.is_kiwi {
                    if ui.small_button("CW band view").clicked() {
                        let full_span = self.plot_full_span_hz();
                        let max_zoom = self.plot_max_zoom_out();
                        let segment = self.default_cw_segment_hz();
                        self.plot.plot_view
                            .zoom_to_cw_segment(segment, full_span, max_zoom);
                    }
                }
            });
            ui.add_space(4.0);
            scroll_slider_f32_step(ui, &mut self.display.pan_step_hz, 50.0..=5000.0, "Pan step (Hz)", 50.0);
            scroll_slider_f32_step(
                ui,
                &mut self.display.pan_step_fast_hz,
                500.0..=50_000.0,
                "Fast pan step (Hz)",
                500.0,
            );
            self.display.pan_step_fast_hz = self.display.pan_step_fast_hz.max(self.display.pan_step_hz);
            if self.radio.is_kiwi {
                toggle(
                    ui,
                    &mut self.display.show_band_overview,
                    "Band overview minimap (M)",
                );
            }
            let floor_db = self.display.ref_db - self.display.range_db;
            ui.label(
                egui::RichText::new(format!(
                    "Floor {:.0} dB · Ref {:.0} dB · Range {:.0} dB",
                    floor_db, self.display.ref_db, self.display.range_db
                ))
                .small()
                .color(MUTED),
            );
            ui.horizontal(|ui| {
                if ui
                    .button("Auto levels")
                    .on_hover_text("Set Ref/Range once from the live spectrum")
                    .clicked()
                {
                    self.display.display_levels_initialized = false;
                    self.update_display_levels();
                }
                ui.toggle_value(
                    &mut self.display.display_auto_track,
                    "Track continuously",
                )
                .on_hover_text(
                    "Keep adjusting Ref/Range as the band changes — RF gain will not change \
                     waterfall brightness while this is on",
                );
            });
            if scroll_slider_f32(ui, &mut self.display.ref_db, -120.0..=20.0, "Ref dB").changed() {
                self.display.display_levels_initialized = true;
                self.display.display_auto_track = false;
                self.plot.force_texture_full = true;
                self.plot.textures_dirty = true;
            }
            if scroll_slider_f32(ui, &mut self.display.range_db, 12.0..=80.0, "Range dB").changed() {
                self.display.display_levels_initialized = true;
                self.display.display_auto_track = false;
                self.plot.force_texture_full = true;
                self.plot.textures_dirty = true;
            }
            scroll_slider_f32(ui, &mut self.display.smooth_alpha, 0.05..=0.45, "Spectrum smooth");
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Waterfall avg").small().color(MUTED));
                for (label, n) in [("None", 1_u8), ("2×", 2), ("4×", 4)] {
                    if ui
                        .selectable_label(self.display.waterfall_avg == n, label)
                        .on_hover_text("Time-average consecutive FFT rows in the waterfall")
                        .clicked()
                    {
                        self.display.waterfall_avg = n;
                        self.plot.force_texture_full = true;
                        self.plot.textures_dirty = true;
                    }
                }
            });
        });
    }



}
