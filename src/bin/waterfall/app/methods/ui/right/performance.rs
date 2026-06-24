// `ui/right/performance` — FFT / decimation / FPS.

    fn performance_section(&mut self, ui: &mut egui::Ui) {
        collapsible_section(ui, "perf", "Performance", None, false, |ui| {
            ui.checkbox(&mut self.fft_auto, "Auto FFT size (wideband)");
            ui.checkbox(
                &mut self.full_drain_spectrum,
                "Full-drain spectrum (wideband, more CPU)",
            )
            .on_hover_text(
                "FFT every drained IQ sample instead of the recent tail only. \
                 Row budget still adapts to CPU headroom.",
            );
            if self.fft_auto {
                let rate = self.stats.spectrum_rate;
                let bin = rate / self.stats.spectrum_fft.max(1) as f32;
                let zoom_note = if self.stats.spectrum_zoomed {
                    format!(" ×{} zoom", self.stats.spectrum_decim)
                } else {
                    String::new()
                };
                stat_row(
                    ui,
                    "FFT",
                    format!(
                        "{} @ {:.1} kS/s (~{bin:.1} Hz/bin){zoom_note}",
                        self.stats.spectrum_fft,
                        rate / 1000.0
                    ),
                );
            } else {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("FFT").small().color(MUTED));
                    for n in [2048usize, 4096, 8192, 16_384, 32_768, 65_536] {
                        if ui.selectable_label(self.fft_size == n, n.to_string()).clicked() {
                            self.fft_size = n;
                        }
                    }
                });
            }

            let mut dec = self.cw.decimation as i32;
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Decimation").small().color(MUTED));
                ui.add(egui::DragValue::new(&mut dec).range(0..=64).speed(1));
                ui.label(egui::RichText::new(if dec == 0 { "auto" } else { "manual" }).small().color(MUTED));
            });
            self.cw.decimation = dec.max(0) as u32;
            let factor = if self.cw.decimation == 0 {
                decimation_factor(self.sample_rate)
            } else {
                self.cw.decimation as usize
            }
            .max(1);
            stat_row(ui, "Audio rate", format!("{:.1} kHz", self.sample_rate / factor as f32 / 1000.0));

            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Decim anti-alias").small().color(MUTED));
                if ui
                    .selectable_label(
                        self.cw.decim_filter == ChannelFilterKind::LinearFir,
                        "FIR",
                    )
                    .on_hover_text("Gaussian FIR before integer decimation (default)")
                    .clicked()
                {
                    self.cw.decim_filter = ChannelFilterKind::LinearFir;
                }
                if ui
                    .selectable_label(
                        self.cw.decim_filter == ChannelFilterKind::Iir2Pole,
                        "IIR 2-pole",
                    )
                    .on_hover_text("Biquad lowpass — ingress + channel decimator")
                    .clicked()
                {
                    self.cw.decim_filter = ChannelFilterKind::Iir2Pole;
                }
            });

            let mut fps = self.target_fps as f32;
            if scroll_slider_f32(ui, &mut fps, 10.0..=60.0, "Target FPS").changed() {
                self.target_fps = fps.round() as u32;
            }
            if self.is_wideband() && self.skimmer_enabled {
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
            if eff_sk.max_channels < self.skimmer.max_channels {
                ui.label(
                    egui::RichText::new(format!(
                        "Skimmer channels capped at {} on wideband",
                        eff_sk.max_channels
                    ))
                    .small()
                    .color(MUTED),
                );
            }

            ui.separator();
            stat_row(ui, "IQ / pump", self.stats.last_drain.to_string());
            stat_row(ui, "Decoders", self.skimmer_channels.to_string());
            if let Some(name) = &self.stats.audio_device {
                stat_row(ui, "Audio out", name.clone());
            }
        });
    }


