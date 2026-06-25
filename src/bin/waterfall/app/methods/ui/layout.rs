use crate::app::WaterfallApp;
use crate::app::prelude::*;

impl WaterfallApp {

    pub(crate) fn side_panel_scroll(&mut self, ui: &mut egui::Ui, mut body: impl FnMut(&mut Self, &mut egui::Ui)) {
        let panel_w = ui.max_rect().width();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_min_width(panel_w);
                body(self, ui);
            });
    }



    pub(crate) fn left_panel(&mut self, ui: &mut egui::Ui) {
        self.side_panel_scroll(ui, |this, ui| {
            if this.chrome.show_smeter {
                this.smeter_card(ui);
            }
            if !this.chrome.show_left {
                return;
            }
            this.frequency_card(ui);
            this.rf_front_end_card(ui);
            this.receive_chain_card(ui);
        });
    }



    pub(crate) fn right_panel(&mut self, ui: &mut egui::Ui) {
        self.side_panel_scroll(ui, |this, ui| {
            this.af_tuning_card(ui);
            this.cw_demod_card(ui);
            this.display_section(ui);
            this.spot_display_section(ui);
            collapsible_section(ui, "skimmer-settings", "Skimmer settings", None, false, |ui| {
                this.skimmer_settings_body(ui);
            });
            collapsible_section(ui, "audio", "Audio", None, false, |ui| {
                this.audio_card_body(ui);
            });
            this.performance_section(ui);
        });
    }


}
