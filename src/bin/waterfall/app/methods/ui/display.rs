use crate::app::WaterfallApp;
use crate::app::prelude::*;

const SPECTRUM_FFT_PRESETS: [usize; 6] = [2048, 4096, 8192, 16_384, 32_768, 65_536];

impl WaterfallApp {

    /// FFT size / RBW controls shared by Display and Performance panels.
    pub(crate) fn spectrum_fft_resolution_controls(&mut self, ui: &mut egui::Ui) {
        if ui
            .checkbox(&mut self.display.fft_auto, "Auto FFT size")
            .on_hover_text(
                "Choose FFT size for ~8 Hz bins at the current spectrum rate. \
                 Disable to pick a fixed size for finer frequency resolution (more CPU).",
            )
            .changed()
        {
            self.plot.waterfall.force_texture_full = true;
            self.plot.waterfall.textures_dirty = true;
        }

        if self.display.fft_auto {
            let effective = self.engine_ui.stats.spectrum_fft;
            if effective > 0 {
                ui.label(
                    egui::RichText::new(format!("Effective FFT {effective}"))
                        .small()
                        .color(MUTED),
                );
            }
        } else {
            ui.label(egui::RichText::new("FFT size").small().color(MUTED));
            let mut size = self.display.fft_size;
            egui::ComboBox::from_id_salt("spectrum_fft_size")
                .selected_text(size.to_string())
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    for n in SPECTRUM_FFT_PRESETS {
                        ui.selectable_value(&mut size, n, n.to_string());
                    }
                });
            if size != self.display.fft_size {
                self.display.fft_size = size;
                self.plot.waterfall.force_texture_full = true;
                self.plot.waterfall.textures_dirty = true;
            }
        }

        let (fft, rate, hop) = self.spectrum_chain_metrics();
        let rbw = hfsdr::bin_width_hz(rate, fft);
        let overlap = if fft > 0 {
            (1.0 - hop as f32 / fft as f32) * 100.0
        } else {
            0.0
        };
        let row_s = hop as f32 / rate.max(1.0);
        ui.label(
            egui::RichText::new(format!(
                "RBW {rbw:.1} Hz · Overlap {overlap:.0}% · Row {row_s:.2} s"
            ))
            .small()
            .color(MUTED),
        );
    }

    fn spectrum_chain_metrics(&self) -> (usize, f32, usize) {
        let stats = &self.engine_ui.stats;
        if matches!(self.engine_ui.conn_state, ConnState::Streaming) {
            let fft = stats.spectrum_fft.max(1);
            let rate = stats.spectrum_rate.max(1.0);
            let hop = hfsdr::spectrum_hop(fft, stats.sample_rate.max(1.0));
            return (fft, rate, hop);
        }
        let iq_rate = self.iq_passband_hz().max(1.0);
        let (_, fft, eff) = hfsdr::spectrum_plan(
            iq_rate,
            self.display.fft_size,
            self.display.fft_auto,
            iq_rate,
        );
        let hop = hfsdr::spectrum_hop(fft, iq_rate);
        (fft, eff.max(1.0), hop)
    }

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
                popup_section(ui, "View", Some("Spectrum/waterfall zoom and span"), |ui| {
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
                                self.plot
                                    .plot_view
                                    .zoom_to_cw_segment(segment, full_span, max_zoom);
                            }
                        }
                    });
                });

                popup_section(ui, "Keyboard pan", Some("Arrow-key tune and pan steps"), |ui| {
                    scroll_slider_f32_step(
                        ui,
                        &mut self.display.pan_step_hz,
                        50.0..=5000.0,
                        "Pan step (Hz)",
                        50.0,
                    );
                    scroll_slider_f32_step(
                        ui,
                        &mut self.display.pan_step_fast_hz,
                        500.0..=50_000.0,
                        "Fast pan step (Hz)",
                        500.0,
                    );
                    self.display.pan_step_fast_hz =
                        self.display.pan_step_fast_hz.max(self.display.pan_step_hz);
                    if self.radio.is_kiwi {
                        toggle(
                            ui,
                            &mut self.display.show_band_overview,
                            "Band overview minimap (M)",
                        );
                    }
                });

                popup_section(ui, "Spectrum levels", Some("dB scale for trace and waterfall"), |ui| {
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
                        ui.toggle_value(&mut self.display.display_auto_track, "Track continuously")
                            .on_hover_text(
                                "Keep adjusting Ref/Range as the band changes — RF gain will not change \
                                 waterfall brightness while this is on",
                            );
                    });
                    if scroll_slider_f32(ui, &mut self.display.ref_db, -120.0..=20.0, "Ref dB").changed() {
                        self.display.display_levels_initialized = true;
                        self.display.display_auto_track = false;
                        self.plot.waterfall.force_texture_full = true;
                        self.plot.waterfall.textures_dirty = true;
                    }
                    if scroll_slider_f32(ui, &mut self.display.range_db, 12.0..=80.0, "Range dB").changed()
                    {
                        self.display.display_levels_initialized = true;
                        self.display.display_auto_track = false;
                        self.plot.waterfall.force_texture_full = true;
                        self.plot.waterfall.textures_dirty = true;
                    }
                    scroll_slider_f32(ui, &mut self.display.smooth_alpha, 0.05..=0.45, "Spectrum smooth");
                });

                popup_section(ui, "Waterfall", None, |ui| {
                    self.spectrum_fft_resolution_controls(ui);

                    let avg_sel = match self.display.waterfall_avg {
                        2 => 1,
                        4 => 2,
                        _ => 0,
                    };
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Time average").small().color(MUTED));
                        if let Some(i) = segment_choice(ui, "wf_avg", avg_sel, &["None", "2×", "4×"]) {
                            self.display.waterfall_avg = match i {
                                1 => 2,
                                2 => 4,
                                _ => 1,
                            };
                            self.plot.waterfall.force_texture_full = true;
                            self.plot.waterfall.textures_dirty = true;
                        }
                    });

                    ui.label(egui::RichText::new("FFT window").small().color(MUTED));
                    let mut window = self.display.spectrum_window;
                    let window_resp = egui::ComboBox::from_id_salt("spectrum_fft_window")
                        .selected_text(window.label())
                        .width(ui.available_width())
                        .show_ui(ui, |ui| {
                            for kind in FftWindowKind::ALL {
                                ui.selectable_value(&mut window, kind, kind.label());
                            }
                        });
                    attach_rich_tooltip(
                        &window_resp.response,
                        Some("FFT window"),
                        &[
                            ("Spectral leakage", ACCENT),
                            (
                                "Tapers each FFT frame before the transform. Steeper windows \
                                 (Blackman, Blackman-Harris) reduce sidelobes but widen peaks; \
                                 Rectangular has the sharpest peaks but most leakage.",
                                MUTED,
                            ),
                            ("Independent of CW filter", OK),
                            (
                                "This only affects the panadapter/waterfall display — not the \
                                 channel bandpass used for demod.",
                                MUTED,
                            ),
                        ],
                    );
                    if window != self.display.spectrum_window {
                        self.display.spectrum_window = window;
                        self.plot.waterfall.force_texture_full = true;
                        self.plot.waterfall.textures_dirty = true;
                    }
                    if self.display.spectrum_window == FftWindowKind::Kaiser {
                        if scroll_slider_f32(
                            ui,
                            &mut self.display.spectrum_kaiser_beta,
                            MIN_KAISER_BETA..=MAX_KAISER_BETA,
                            "Kaiser β",
                        )
                        .changed()
                        {
                            self.plot.waterfall.force_texture_full = true;
                            self.plot.waterfall.textures_dirty = true;
                        }
                    }
                });
            },
        );
    }
}
