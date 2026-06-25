use crate::app::WaterfallApp;
use crate::app::prelude::*;

impl WaterfallApp {

    pub(crate) fn console_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Log").strong());
            if ui.button("Clear").clicked() {
                log::clear();
            }
        });
        egui::ScrollArea::vertical()
            .max_height(140.0)
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for line in log::entries() {
                    ui.label(
                        egui::RichText::new(line)
                            .monospace()
                            .size(11.0)
                            .color(MUTED),
                    );
                }
            });
    }


}
