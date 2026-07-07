use crate::app::WaterfallApp;
use crate::app::prelude::*;

impl WaterfallApp {
    pub(crate) fn history_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(
                &mut self.chrome.bottom_panel_view,
                BottomPanelView::CallsignLog,
                "Callsign log",
            );
            ui.selectable_value(
                &mut self.chrome.bottom_panel_view,
                BottomPanelView::Decoder,
                "Decoder",
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let peaks = self.skimmer_ui.skimmer_channels;
                let keyed = self
                    .skimmer_ui
                    .skimmer_decode_channels
                    .iter()
                    .filter(|c| c.keyed)
                    .count();
                let spots = self
                    .skimmer_ui
                    .skimmer_spots
                    .iter()
                    .filter(|s| s.callsign.is_some())
                    .count();
                ui.label(
                    egui::RichText::new(format!("{spots} calls · {keyed} keyed · {peaks} peaks"))
                        .small()
                        .color(MUTED),
                );
            });
        });
        ui.add_space(4.0);

        match self.chrome.bottom_panel_view {
            BottomPanelView::CallsignLog => self.callsign_log_panel(ui),
            BottomPanelView::Decoder => self.decoder_panel(ui),
        }
    }

    fn callsign_log_panel(&mut self, ui: &mut egui::Ui) {
        let entries = self.callsign_log_entries();
        let scroll_h = ui.available_height().max(24.0);
        if entries.is_empty() {
            let hint = if !self.skimmer_ui.skimmer_enabled {
                "Enable skimmer to decode callsigns on the band."
            } else if !self.connection_session_live() {
                "Connect and wait for STREAMING — callsigns appear after real CW is copied."
            } else if self.skimmer_ui.skimmer_decode_channels.iter().any(|c| c.keyed) {
                "Keyed CW heard — waiting for CQ/DE + callsign in the exchange."
            } else if self.skimmer_ui.skimmer_channels > 0 {
                "Peaks tracked — decoder opens when keyed CW is detected."
            } else {
                "No callsign decodes in the last 10 minutes."
            };
            ui.label(egui::RichText::new(hint).small().color(MUTED));
            ui.allocate_space(egui::vec2(0.0, ui.available_height()));
            return;
        }

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .max_height(scroll_h)
            .show(ui, |ui| {
                for spot in entries {
                    let call = spot.callsign.as_deref().unwrap_or("—");
                    let freq_khz = spot.frequency_hz / 1e3;
                    let age_s = spot.age().as_secs();
                    let age_txt = if age_s < 60 {
                        format!("{age_s}s ago")
                    } else {
                        format!("{}m ago", age_s / 60)
                    };
                    let kind_txt = match spot.kind {
                        SpotKind::CallingCq => "CQ",
                        SpotKind::Answering => "Ans",
                        SpotKind::Heard => "Heard",
                    };
                    let frame = egui::Frame::new()
                        .fill(Color32::from_rgb(32, 38, 52))
                        .corner_radius(egui::CornerRadius::same(6))
                        .inner_margin(8.0)
                        .stroke(egui::Stroke::new(1.0, Color32::from_rgb(55, 65, 85)));
                    frame.show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(call).strong().color(OK));
                            ui.label(
                                egui::RichText::new(format!("{freq_khz:.1} kHz"))
                                    .monospace()
                                    .small(),
                            );
                            ui.label(egui::RichText::new(kind_txt).small().color(MUTED));
                            ui.label(
                                egui::RichText::new(format!("+{:.0} dB", spot.snr_db))
                                    .small()
                                    .color(MUTED),
                            );
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui
                                    .small_button("Tune")
                                    .on_hover_text("Tune receiver to this spot")
                                    .clicked()
                                {
                                    self.tune_to_hz(spot.frequency_hz);
                                }
                                ui.label(egui::RichText::new(age_txt).small().color(MUTED));
                            });
                        });
                    });
                    ui.add_space(4.0);
                }
            });
    }

    fn decoder_panel(&mut self, ui: &mut egui::Ui) {
        let channels = self.skimmer_ui.skimmer_decode_channels.clone();
        let scroll_h = ui.available_height().max(24.0);
        if channels.is_empty() {
            let hint = if !self.skimmer_ui.skimmer_enabled {
                "Enable skimmer to open decoder channels."
            } else if !self.connection_session_live() {
                "Connect first — decoder channels appear when IQ is streaming."
            } else if !self.skimmer_runtime_enabled() {
                "Skimmer needs Process IQ ≤96 kHz (or Kiwi / IQ playback)."
            } else if self.skimmer_ui.skimmer_channels > 0 {
                "Peaks on the band — decoder text appears when keyed CW is detected."
            } else {
                "No spectrum peaks above skimmer SNR — lower Peak min SNR in skimmer settings."
            };
            ui.label(egui::RichText::new(hint).small().color(MUTED));
            ui.allocate_space(egui::vec2(0.0, ui.available_height()));
            return;
        }

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .max_height(scroll_h)
            .show(ui, |ui| {
                for ch in channels {
                    let freq_khz = ch.frequency_hz / 1e3;
                    let text = if ch.text.is_empty() {
                        "…".to_string()
                    } else {
                        ch.text.clone()
                    };
                    let frame = egui::Frame::new()
                        .fill(Color32::from_rgb(28, 34, 48))
                        .corner_radius(egui::CornerRadius::same(6))
                        .inner_margin(8.0)
                        .stroke(egui::Stroke::new(1.0, Color32::from_rgb(50, 60, 80)));
                    frame.show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format!("{freq_khz:.1} kHz"))
                                    .monospace()
                                    .strong(),
                            );
                            ui.label(
                                egui::RichText::new(format!("+{:.0} dB", ch.snr_db))
                                    .small()
                                    .color(MUTED),
                            );
                            ui.label(
                                egui::RichText::new(format!("{:.0} WPM", ch.wpm))
                                    .small()
                                    .color(MUTED),
                            );
                            if ch.keyed {
                                ui.label(egui::RichText::new("KEYED").small().color(OK));
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button("Tune").clicked() {
                                    self.tune_to_hz(ch.frequency_hz);
                                }
                            });
                        });
                        ui.label(
                            egui::RichText::new(text)
                                .monospace()
                                .small()
                                .color(Color32::from_rgb(200, 210, 230)),
                        );
                    });
                    ui.add_space(4.0);
                }
            });
    }
}
