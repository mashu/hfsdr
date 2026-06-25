use crate::app::WaterfallApp;
use crate::app::prelude::*;

impl WaterfallApp {

    pub(crate) fn performance_section(&mut self, ui: &mut egui::Ui) {
        collapsible_section(ui, "perf", "Performance", None, false, |ui| {
            popup_section(ui, "FFT & spectrum", Some("Resolution vs CPU on wideband sources"), |ui| {
                ui.checkbox(&mut self.display.fft_auto, "Auto FFT size (wideband)");
                ui.checkbox(
                    &mut self.display.full_drain_spectrum,
                    "Full-drain spectrum (wideband, more CPU)",
                )
                .on_hover_text(
                    "FFT every drained IQ sample instead of the recent tail only. \
                     Row budget still adapts to CPU headroom.",
                );
                ui.checkbox(&mut self.display.perf_trace, "Pipeline profiling")
                    .on_hover_text(
                        "Per-pump stage timings below. Also enabled when HFSDR_PERF=1 is set in the environment.",
                    );
                if self.display.fft_auto {
                    let rate = self.engine_ui.stats.spectrum_rate;
                    let bin = rate / self.engine_ui.stats.spectrum_fft.max(1) as f32;
                    stat_row(ui, "FFT size", self.engine_ui.stats.spectrum_fft);
                    stat_row(ui, "Spectrum rate", format!("{:.1} kS/s", rate / 1000.0));
                    stat_row(ui, "Bin width", format!("{bin:.1} Hz"));
                    if self.engine_ui.stats.spectrum_zoomed {
                        stat_row(
                            ui,
                            "Zoom decimation",
                            format!("×{}", self.engine_ui.stats.spectrum_decim),
                        );
                    }
                } else {
                    ui.label(egui::RichText::new("FFT size").small().color(MUTED));
                    ui.horizontal_wrapped(|ui| {
                        for n in [2048usize, 4096, 8192, 16_384, 32_768, 65_536] {
                            if ui.selectable_label(self.display.fft_size == n, n.to_string()).clicked() {
                                self.display.fft_size = n;
                            }
                        }
                    });
                }
            });

            popup_section(ui, "Decimation", Some("IQ rate into the CW channel chain"), |ui| {
                let mut dec = self.radio.cw.decimation as i32;
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Factor").small().color(MUTED));
                    ui.add(egui::DragValue::new(&mut dec).range(0..=64).speed(1));
                    ui.label(
                        egui::RichText::new(if dec == 0 { "auto" } else { "manual" })
                            .small()
                            .color(MUTED),
                    );
                });
                self.radio.cw.decimation = dec.max(0) as u32;
                let factor = if self.radio.cw.decimation == 0 {
                    decimation_factor(self.radio.sample_rate)
                } else {
                    self.radio.cw.decimation as usize
                }
                .max(1);
                stat_row(
                    ui,
                    "Audio rate",
                    format!("{:.1} kHz", self.radio.sample_rate / factor as f32 / 1000.0),
                );
                let arch_sel = if self.radio.cw.decim_filter == ChannelFilterKind::LinearFir {
                    0
                } else {
                    1
                };
                ui.label(egui::RichText::new("Anti-alias").small().color(MUTED));
                if let Some(i) = segment_choice(ui, "decim_aa", arch_sel, &["FIR", "IIR"]) {
                    self.radio.cw.decim_filter = if i == 0 {
                        ChannelFilterKind::LinearFir
                    } else {
                        ChannelFilterKind::Iir2Pole
                    };
                }
            });

            popup_section(ui, "Repaint", Some("UI refresh rate and wideband limits"), |ui| {
                let mut fps = self.display.target_fps as f32;
                if scroll_slider_f32(ui, &mut fps, 10.0..=60.0, "Target FPS").changed() {
                    self.display.target_fps = fps.round() as u32;
                }
                if self.is_wideband() && self.skimmer_ui.skimmer_enabled {
                    ui.label(
                        egui::RichText::new(format!(
                            "Repaint capped at {} FPS while wideband + skimmer",
                            self.effective_target_fps()
                        ))
                        .small()
                        .color(MUTED),
                    );
                }
                let eff_sk = self.effective_skimmer();
                if eff_sk.max_channels < self.skimmer_ui.skimmer.max_channels {
                    ui.label(
                        egui::RichText::new(format!(
                            "Skimmer channels capped at {} on wideband",
                            eff_sk.max_channels
                        ))
                        .small()
                        .color(MUTED),
                    );
                }
            });

            popup_section(ui, "Runtime", None, |ui| {
                stat_row(ui, "IQ / pump", self.engine_ui.stats.last_drain.to_string());
                stat_row(ui, "Decoders", self.skimmer_ui.skimmer_channels.to_string());
                stat_row(
                    ui,
                    "Effective rate",
                    format!(
                        "{:.1} kS/s",
                        self.engine_ui.stats.effective_sps / 1000.0
                    ),
                );
                if self.engine_ui.stats.pipeline.dual_ring {
                    stat_row(ui, "IQ path", "dual ring (raw + decim)");
                }
                if let Some(name) = &self.engine_ui.stats.audio_device {
                    stat_row(ui, "Audio out", name.clone());
                }
            });

            if self.display.perf_trace || self.engine_ui.stats.pipeline_avg.measured_total_ns() > 0
            {
                popup_section(
                    ui,
                    "Pipeline profile",
                    Some("Smoothed per-pump CPU time (engine thread)"),
                    |ui| {
                        let m = &self.engine_ui.stats.pipeline_avg;
                        let total_us = m.measured_total_ns() as f64 / 1000.0;
                        stat_row(ui, "Total", format!("{total_us:.0} µs/pump"));
                        if m.dual_ring {
                            stat_row(ui, "Ingress", "dual ring (off hot path)");
                        }
                        for (name, ns) in m.stage_rows() {
                            if ns == 0 {
                                continue;
                            }
                            let pct = ns as f64 / m.measured_total_ns().max(1) as f64 * 100.0;
                            stat_row(
                                ui,
                                name,
                                format!("{:.0}% ({:.0} µs)", pct, ns as f64 / 1000.0),
                            );
                        }
                        if m.iq_dropped_catchup > 0 {
                            stat_row(
                                ui,
                                "IQ catch-up drops",
                                m.iq_dropped_catchup.to_string(),
                            );
                        }
                        if m.decim_ring_dropped > 0 {
                            stat_row(
                                ui,
                                "Decim ring drops",
                                m.decim_ring_dropped.to_string(),
                            );
                        }
                        stat_row(ui, "FFT rows / pump", m.fft_rows.to_string());
                    },
                );
            }
        });
    }
}
