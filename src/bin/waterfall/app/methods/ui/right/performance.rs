use crate::app::WaterfallApp;
use crate::app::prelude::*;

impl WaterfallApp {

    pub(crate) fn performance_section(&mut self, ui: &mut egui::Ui) {
        collapsible_section(ui, "perf", "Performance", None, false, |ui| {
            let stats = self.engine_ui.stats.clone();
            let m = stats.pipeline_avg.clone();

            let nominal = stats.sample_rate.max(1.0);
            let eff_pct = (stats.effective_sps / nominal * 100.0).clamp(0.0, 999.0);
            let slow = stats.slow || eff_pct < 70.0;
            let wide_device = self.is_wideband_device();

            if wide_device {
                ui.label(
                    egui::RichText::new(
                        "Wideband device IQ: CW demod dominates CPU. Set IQ process rate to \
                         192 kHz (Connection) and avoid 768 kHz unless needed.",
                    )
                    .small()
                    .color(if slow { egui::Color32::from_rgb(255, 180, 80) } else { MUTED }),
                );
            } else if nominal <= 96_000.0 && !self.radio.cw.full_demod {
                ui.label(
                    egui::RichText::new(
                        "Listen demod uses the last 2048 IQ samples per pump on catch-up. \
                         Enable Full demod drain (below) for contest copy.",
                    )
                    .small()
                    .color(MUTED),
                );
            }

            popup_section(
                ui,
                "Throughput & drops",
                Some("Headroom vs nominal IQ rate — drops mean the engine is falling behind"),
                |ui| {
                    let eff_color = if eff_pct >= 85.0 {
                        MUTED
                    } else if eff_pct >= 70.0 {
                        egui::Color32::from_rgb(255, 200, 80)
                    } else {
                        egui::Color32::from_rgb(255, 100, 80)
                    };
                    stat_row(
                        ui,
                        "Effective IQ rate",
                        format!(
                            "{:.1} kS/s ({eff_pct:.0}% of {:.0} k)",
                            stats.effective_sps / 1000.0,
                            nominal / 1000.0
                        ),
                    );
                    ui.colored_label(eff_color, "↑ primary health metric");
                    if stats.slow {
                        ui.colored_label(
                            egui::Color32::from_rgb(255, 100, 80),
                            "Link slow — IQ ring catch-up active",
                        );
                    }
                    stat_row(ui, "Source drops", stats.dropped.to_string());
                    if m.iq_dropped_catchup > 0 {
                        stat_row(
                            ui,
                            "Engine catch-up drops",
                            m.iq_dropped_catchup.to_string(),
                        );
                    }
                    if m.raw_ring_dropped > 0 {
                        stat_row(ui, "Raw ring drops (bridge)", m.raw_ring_dropped.to_string());
                    }
                    if m.decim_ring_dropped > 0 {
                        stat_row(ui, "Decim ring drops (bridge)", m.decim_ring_dropped.to_string());
                    }
                },
            );

            popup_section(ui, "FFT & spectrum", Some("Resolution vs CPU on wideband sources"), |ui| {
                self.spectrum_fft_resolution_controls(ui);
                ui.checkbox(
                    &mut self.display.full_drain_spectrum,
                    "Full-drain spectrum (wideband, more CPU)",
                )
                .on_hover_text(
                    "FFT every drained IQ sample instead of the recent tail only. \
                     Full-span FFT at 768 kHz is very expensive even with auto FFT cap.",
                );
                ui.checkbox(&mut self.display.perf_trace, "Pipeline profiling")
                    .on_hover_text(
                        "Per-pump stage timings below. Also enabled when HFSDR_PERF=1 is set in the environment.",
                    );
                if self.display.fft_auto && matches!(self.engine_ui.conn_state, ConnState::Streaming) {
                    let stats = &self.engine_ui.stats;
                    if stats.spectrum_zoomed {
                        stat_row(
                            ui,
                            "Zoom decimation",
                            format!("×{}", stats.spectrum_decim),
                        );
                    }
                    stat_row(
                        ui,
                        "Spectrum rate",
                        format!("{:.1} kS/s", stats.spectrum_rate / 1000.0),
                    );
                }
            });

            popup_section(ui, "Decimation", Some("IQ rate into the CW channel chain"), |ui| {
                ui.checkbox(
                    &mut self.radio.cw.full_demod,
                    "Full demod drain (contest)",
                )
                .on_hover_text(
                    "Every IQ sample drained each pump goes through listen demod — no tail cap \
                     on catch-up. Filter state stays continuous; no missed dits under ring pressure. \
                     Off saves CPU when the ring over-fills (may clip audio during catch-up).",
                );
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
                if let Some(i) = segment_choice_sized(ui, "decim_aa", arch_sel, &["FIR", "IIR"], 36.0) {
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
                let mut wf_rows = self.display.waterfall_rows_per_frame as f32;
                if scroll_slider_f32(ui, &mut wf_rows, 1.0..=8.0, "Waterfall rows / frame").changed()
                {
                    self.display.waterfall_rows_per_frame = wf_rows.round() as u32;
                }
                ui.label(
                    egui::RichText::new(
                        "Also under Display → Waterfall. 1 row/frame = slowest smooth scroll.",
                    )
                    .small()
                    .color(MUTED),
                );
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
                stat_row(ui, "IQ / pump", stats.last_drain.to_string());
                stat_row(ui, "Decoders", self.skimmer_ui.skimmer_channels.to_string());
                if stats.pipeline.dual_ring {
                    stat_row(ui, "IQ path", "dual ring (raw + decim)");
                }
                if let Some(name) = &stats.audio_device {
                    stat_row(ui, "Audio out", name.clone());
                }
            });

            let wf = &self.plot.waterfall.perf;
            if matches!(self.engine_ui.conn_state, ConnState::Streaming) {
                popup_section(
                    ui,
                    "Waterfall (UI thread)",
                    Some("Ring texture + mesh; interval jitter explains jumpy scroll"),
                    |ui| {
                        stat_row(
                            ui,
                            "Pending rows",
                            wf.rows_pending.to_string(),
                        );
                        stat_row(
                            ui,
                            "Applied last frame",
                            wf.rows_applied_last.to_string(),
                        );
                        stat_row(
                            ui,
                            "Rows / frame cap",
                            wf.rows_per_frame_cap.to_string(),
                        );
                        if wf.row_interval_ms > 0.0 {
                            stat_row(
                                ui,
                                "Row interval (EMA)",
                                format!("{:.0} ms", wf.row_interval_ms),
                            );
                        }
                        stat_row(
                            ui,
                            "Compose",
                            format!("{:.0} µs", wf.compose_ns as f64 / 1000.0),
                        );
                        stat_row(
                            ui,
                            "GPU upload",
                            format!("{:.0} µs", wf.upload_ns as f64 / 1000.0),
                        );
                        stat_row(
                            ui,
                            "Uploads",
                            format!("full {} · partial {}", wf.uploads_full, wf.uploads_partial),
                        );
                        stat_row(ui, "Ring head", self.plot.waterfall.viewport_row_head.to_string());
                        if wf.rows_pending > wf.rows_per_frame_cap.saturating_mul(4) {
                            ui.label(
                                egui::RichText::new(
                                    "Waterfall backlog — raise scroll speed or FPS if you want \
                                     to stay closer to live.",
                                )
                                .small()
                                .color(egui::Color32::from_rgb(255, 200, 80)),
                            );
                        }
                    },
                );
            }

            if self.display.perf_trace || m.measured_total_ns() > 0 {
                popup_section(
                    ui,
                    "Pipeline profile",
                    Some("Smoothed per-pump CPU time (engine thread)"),
                    |ui| {
                        let total_us = m.measured_total_ns() as f64 / 1000.0;
                        stat_row(ui, "Total", format!("{total_us:.0} µs/pump"));
                        if m.demod_ns * 100 > m.measured_total_ns().saturating_mul(80) {
                            ui.label(
                                egui::RichText::new(
                                    "Demod is the bottleneck — run: cargo run --release --bin engine-bench demod",
                                )
                                .small()
                                .color(MUTED),
                            );
                        }
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
                        stat_row(ui, "FFT rows / pump", m.fft_rows.to_string());
                    },
                );
            }
        });
    }
}
