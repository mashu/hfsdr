// `ui/spots/skimmer` — skimmer decoder settings.

    fn skimmer_settings_body(&mut self, ui: &mut egui::Ui) {
        if self.skimmer_enabled {
            stat_row(ui, "Decoders", self.skimmer_channels.to_string());
        }
        self.scp_section(ui);

        section_heading(ui, "Decoder & channel DSP");
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Algorithm").small().color(MUTED));
                let bigram = ui.selectable_label(
                    self.skimmer.decoder == SkimmerDecoderKind::Bigram,
                    "Bigram beam",
                );
                attach_rich_tooltip(
                    &bigram,
                    Some("Decoder"),
                    &[
                        ("Bigram beam", ACCENT),
                        ("Best copy on pileups.", OK),
                        ("Adaptive", ACCENT),
                        ("Lighter CPU.", MUTED),
                    ],
                );
                if bigram.clicked() {
                    self.skimmer.decoder = SkimmerDecoderKind::Bigram;
                }
                let adaptive = ui.selectable_label(
                    self.skimmer.decoder == SkimmerDecoderKind::Adaptive,
                    "Adaptive",
                );
                attach_rich_tooltip(
                    &adaptive,
                    Some("Decoder"),
                    &[
                        ("Bigram beam", ACCENT),
                        ("Best copy on pileups.", OK),
                        ("Adaptive", ACCENT),
                        ("Lighter CPU.", MUTED),
                    ],
                );
                if adaptive.clicked() {
                    self.skimmer.decoder = SkimmerDecoderKind::Adaptive;
                }
            });
            scroll_slider_f32(ui, &mut self.skimmer.min_snr_db, 6.0..=30.0, "Peak min SNR");
            scroll_slider_f32(ui, &mut self.skimmer.min_decode_snr_db, 6.0..=40.0, "Decode min SNR");
            scroll_slider_f32(ui, &mut self.skimmer.decode_gate_ms, 20.0..=500.0, "Key gate ms");
            scroll_slider_f32(ui, &mut self.skimmer.bucket_hz, 20.0..=200.0, "Bucket Hz");
            let mut sep = self.skimmer.min_separation_bins as i32;
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Peak separation").small().color(MUTED));
                ui.add(egui::DragValue::new(&mut sep).range(1..=32).speed(1));
                ui.label(egui::RichText::new("bins").small().color(MUTED));
            });
            self.skimmer.min_separation_bins = sep as usize;
            let mut max_ch = self.skimmer.max_channels as i32;
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Max decoders").small().color(MUTED));
                ui.add(egui::DragValue::new(&mut max_ch).range(4..=64).speed(1));
            });
            self.skimmer.max_channels = max_ch as usize;
            scroll_slider_f32(
                ui,
                &mut self.skimmer.lpf_cutoff_hz,
                40.0..=800.0,
                "Channel LPF Hz",
            );
            scroll_slider_log_f32(
                ui,
                &mut self.skimmer.target_audio_rate_hz,
                4_000.0..=48_000.0,
                "Target audio rate",
            );
            scroll_slider_f32(
                ui,
                &mut self.skimmer.decoder_params.initial_wpm,
                8.0..=60.0,
                "Initial WPM",
            );
            if self.skimmer.decoder == SkimmerDecoderKind::Bigram {
                let mut beam = self.skimmer.decoder_params.beam_width as i32;
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Beam width").small().color(MUTED));
                    ui.add(egui::DragValue::new(&mut beam).range(1..=64).speed(1));
                });
                self.skimmer.decoder_params.beam_width = beam as usize;
            }
            scroll_slider_f32(
                ui,
                &mut self.skimmer.decoder_params.envelope.thr_low,
                0.05..=0.9,
                "Key thr low",
            );
            scroll_slider_f32(
                ui,
                &mut self.skimmer.decoder_params.envelope.thr_high,
                0.1..=0.99,
                "Key thr high",
            );
            if self.skimmer.decoder_params.envelope.thr_high
                <= self.skimmer.decoder_params.envelope.thr_low
            {
                self.skimmer.decoder_params.envelope.thr_high =
                    self.skimmer.decoder_params.envelope.thr_low + 0.05;
            }
            scroll_slider_f32(
                ui,
                &mut self.skimmer.channel_timeout_secs,
                1.0..=120.0,
                "Channel timeout (s)",
            );
            scroll_slider_f32(
                ui,
                &mut self.skimmer.spot_store_max_age_secs,
                0.0..=600.0,
                "Store max age (s, 0=keep)",
            );
            let mut max_txt = self.skimmer.decoder_params.max_text_chars as i32;
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Max decode chars").small().color(MUTED));
                ui.add(egui::DragValue::new(&mut max_txt).range(16..=256).speed(1));
            });
            self.skimmer.decoder_params.max_text_chars = max_txt as usize;
            toggle(
                ui,
                &mut self.skimmer.require_scp,
                "Require MASTER.SCP match",
            );
    }


