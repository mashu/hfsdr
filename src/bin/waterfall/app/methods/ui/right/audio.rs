use crate::app::WaterfallApp;
use crate::app::prelude::*;

impl WaterfallApp {

    pub(crate) fn audio_card_body(&mut self, ui: &mut egui::Ui) {
        if self.audio.audio_devices.is_empty() {
                ui.colored_label(WARN, "No output devices found");
            } else {
                let selected = self
                    .audio.audio_devices
                    .get(self.audio.selected_audio_device)
                    .map(String::as_str)
                    .unwrap_or("");
                egui::ComboBox::from_label("Output device")
                    .selected_text(selected)
                    .show_ui(ui, |ui| {
                        for (idx, name) in self.audio.audio_devices.iter().enumerate() {
                            ui.selectable_value(&mut self.audio.selected_audio_device, idx, name);
                        }
                    });
                if ui.small_button("Refresh devices").clicked() {
                    self.audio.audio_devices = AudioOutput::list_output_devices();
                    if self.audio.selected_audio_device >= self.audio.audio_devices.len() {
                        self.audio.selected_audio_device = 0;
                    }
                    self.audio.last_audio_device = usize::MAX;
                }
            }
            stage_toggle(
                ui,
                &mut self.audio.audio_enabled,
                "Speakers",
                Some("Spectrum/waterfall keep running when off"),
                Some("Space"),
                Some(&[
                    ("Mute", ACCENT),
                    (
                        "Muting speakers or volume 0 keeps spectrum, waterfall, and skimmer running.",
                        MUTED,
                    ),
                ]),
            );
            scroll_slider_f32(ui, &mut self.audio.volume, 0.0..=4.0, "Volume (- / +)");
            if let Some(name) = &self.engine_ui.stats.audio_device {
                stat_row(ui, "Active", name.clone());
                stat_row(ui, "Rate", format!("{} Hz", self.engine_ui.stats.audio_rate));
            } else {
                ui.colored_label(WARN, "No output device open");
            }
    }



}
