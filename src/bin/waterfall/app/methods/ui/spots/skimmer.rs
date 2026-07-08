use crate::app::WaterfallApp;
use crate::app::prelude::*;

impl WaterfallApp {

    pub(crate) fn skimmer_settings_body(&mut self, ui: &mut egui::Ui) {
        if self.skimmer_ui.skimmer_enabled {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Active decoders").small().color(MUTED));
                ui.label(
                    egui::RichText::new(self.skimmer_ui.skimmer_channels.to_string()).strong(),
                );
            });
        }

        self.scp_body(ui);

        popup_section(ui, "Decoder", Some("How peaks become callsigns"), |ui| {
            let dec_sel = match self.skimmer_ui.skimmer.decoder {
                SkimmerDecoderKind::Bayes => 0,
                SkimmerDecoderKind::Bigram => 1,
                SkimmerDecoderKind::Adaptive => 2,
            };
            ui.label(egui::RichText::new("Algorithm").small().color(MUTED));
            if let Some(i) = segment_choice(
                ui,
                "skim_decoder",
                dec_sel,
                &["Bayesian", "Bigram beam", "Adaptive"],
            ) {
                self.skimmer_ui.skimmer.decoder = match i {
                    0 => SkimmerDecoderKind::Bayes,
                    1 => SkimmerDecoderKind::Bigram,
                    _ => SkimmerDecoderKind::Adaptive,
                };
            }
            let dec_hint = match self.skimmer_ui.skimmer.decoder {
                SkimmerDecoderKind::Bayes => "Statistical model — best weak-signal copy, self-tuning",
                SkimmerDecoderKind::Bigram => "Beam search on threshold keying — higher CPU",
                SkimmerDecoderKind::Adaptive => "Lighter CPU — good for casual browsing",
            };
            ui.label(egui::RichText::new(dec_hint).small().color(MUTED));
            scroll_slider_f32(ui, &mut self.skimmer_ui.skimmer.min_snr_db, 6.0..=30.0, "Peak min SNR");
            scroll_slider_f32(
                ui,
                &mut self.skimmer_ui.skimmer.min_decode_snr_db,
                6.0..=40.0,
                "Decode min SNR",
            );
            toggle(
                ui,
                &mut self.skimmer_ui.skimmer.require_scp,
                "Require MASTER.SCP match",
            );
        });

        popup_section(ui, "Peak finder", Some("How signals are split into channels"), |ui| {
            scroll_slider_f32(
                ui,
                &mut self.skimmer_ui.skimmer.focus_span_hz,
                0.0..=12_000.0,
                "Decode span Hz (0 = all)",
            );
            ui.label(
                egui::RichText::new("Span follows the tuned frequency — bounds CPU use")
                    .small()
                    .color(MUTED),
            );
            scroll_slider_f32(ui, &mut self.skimmer_ui.skimmer.bucket_hz, 20.0..=200.0, "Bucket Hz");
            let mut sep = self.skimmer_ui.skimmer.min_separation_bins as i32;
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Peak separation").small().color(MUTED));
                ui.add(egui::DragValue::new(&mut sep).range(1..=32).speed(1));
                ui.label(egui::RichText::new("bins").small().color(MUTED));
            });
            self.skimmer_ui.skimmer.min_separation_bins = sep as usize;
            let mut max_ch = self.skimmer_ui.skimmer.max_channels as i32;
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Max decoders").small().color(MUTED));
                ui.add(egui::DragValue::new(&mut max_ch).range(4..=64).speed(1));
            });
            self.skimmer_ui.skimmer.max_channels = max_ch as usize;
            scroll_slider_f32(
                ui,
                &mut self.skimmer_ui.skimmer.channel_timeout_secs,
                1.0..=120.0,
                "Channel timeout (s)",
            );
        });

        popup_section(ui, "Channel audio", Some("Per-decoder filtering before the keyer"), |ui| {
            scroll_slider_f32(
                ui,
                &mut self.skimmer_ui.skimmer.lpf_cutoff_hz,
                25.0..=250.0,
                "Channel LPF Hz",
            );
        });

        popup_section(ui, "Key detector", Some("Envelope thresholds and timing"), |ui| {
            scroll_slider_f32(
                ui,
                &mut self.skimmer_ui.skimmer.decoder_params.initial_wpm,
                8.0..=60.0,
                "Initial WPM",
            );
            if self.skimmer_ui.skimmer.decoder != SkimmerDecoderKind::Adaptive {
                let mut beam = self.skimmer_ui.skimmer.decoder_params.beam_width as i32;
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Beam width").small().color(MUTED));
                    ui.add(egui::DragValue::new(&mut beam).range(1..=64).speed(1));
                });
                self.skimmer_ui.skimmer.decoder_params.beam_width = beam as usize;
            }
            scroll_slider_f32(ui, &mut self.skimmer_ui.skimmer.decode_gate_ms, 20.0..=500.0, "Key gate ms");
            if self.skimmer_ui.skimmer.decoder == SkimmerDecoderKind::Bayes {
                ui.label(
                    egui::RichText::new("Key thresholds are estimated from the signal")
                        .small()
                        .color(MUTED),
                );
            } else {
                scroll_slider_f32(
                    ui,
                    &mut self.skimmer_ui.skimmer.decoder_params.envelope.thr_low,
                    0.05..=0.9,
                    "Key thr low",
                );
                scroll_slider_f32(
                    ui,
                    &mut self.skimmer_ui.skimmer.decoder_params.envelope.thr_high,
                    0.1..=0.99,
                    "Key thr high",
                );
                if self.skimmer_ui.skimmer.decoder_params.envelope.thr_high
                    <= self.skimmer_ui.skimmer.decoder_params.envelope.thr_low
                {
                    self.skimmer_ui.skimmer.decoder_params.envelope.thr_high =
                        self.skimmer_ui.skimmer.decoder_params.envelope.thr_low + 0.05;
                }
            }
        });

        popup_section(ui, "Spot storage", Some("How long decodes are kept in memory"), |ui| {
            scroll_slider_f32(
                ui,
                &mut self.skimmer_ui.skimmer.spot_store_max_age_secs,
                0.0..=600.0,
                "Store max age (s, 0 = keep)",
            );
            let mut max_txt = self.skimmer_ui.skimmer.decoder_params.max_text_chars as i32;
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Max decode chars").small().color(MUTED));
                ui.add(egui::DragValue::new(&mut max_txt).range(16..=256).speed(1));
            });
            self.skimmer_ui.skimmer.decoder_params.max_text_chars = max_txt as usize;
        });
    }
}
