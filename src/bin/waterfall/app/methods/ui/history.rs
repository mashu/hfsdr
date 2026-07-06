use crate::app::WaterfallApp;
use crate::app::prelude::*;

impl WaterfallApp {

    pub(crate) fn history_panel(&mut self, ui: &mut egui::Ui) {
        section_heading(ui, "Callsign log (10 min)");
        let center_hz = self.radio.center_khz * 1000.0;
        let annotations: Vec<_> = self.slow.annotations().iter().cloned().collect();
        let scroll_h = ui.available_height().max(24.0);
        if annotations.is_empty() {
            ui.label(
                egui::RichText::new("Decoded callsigns appear here when skimmer is on.")
                    .small()
                    .color(MUTED),
            );
            ui.allocate_space(egui::vec2(0.0, ui.available_height()));
            return;
        }
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .max_height(scroll_h)
            .show(ui, |ui| {
                for ann in annotations {
                    let age = ann.at.elapsed();
                    let freq_khz = center_hz + ann.offset_hz as f64;
                    let age_s = age.as_secs();
                    let age_txt = if age_s < 60 {
                        format!("{age_s}s ago")
                    } else {
                        format!("{}m ago", age_s / 60)
                    };
                    let frame = egui::Frame::new()
                        .fill(Color32::from_rgb(32, 38, 52))
                        .corner_radius(egui::CornerRadius::same(6))
                        .inner_margin(8.0)
                        .stroke(egui::Stroke::new(1.0, Color32::from_rgb(55, 65, 85)));
                    frame.show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(&ann.label).strong().color(OK));
                            ui.label(
                                egui::RichText::new(format!("{freq_khz:.1} kHz"))
                                    .monospace()
                                    .small(),
                            );
                            ui.label(
                                egui::RichText::new(format!("+{:.0} dB", ann.snr_db))
                                    .small()
                                    .color(MUTED),
                            );
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui
                                    .small_button("Tune")
                                    .on_hover_text("Tune receiver to this spot")
                                    .clicked()
                                {
                                    self.tune_to_hz(freq_khz);
                                }
                                ui.label(egui::RichText::new(age_txt).small().color(MUTED));
                            });
                        });
                    });
                    ui.add_space(4.0);
                }
            });
    }


}
