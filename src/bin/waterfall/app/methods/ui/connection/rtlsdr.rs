use crate::app::WaterfallApp;
use crate::app::prelude::*;

impl WaterfallApp {

    #[cfg(feature = "rtlsdr")]
    pub(crate) fn connection_rtlsdr_section(&mut self, ui: &mut egui::Ui) {
        popup_section(ui, "RTL-SDR", None, |ui| {
            egui::Grid::new("connect_rtlsdr_grid")
                .num_columns(2)
                .spacing([8.0, 4.0])
                .min_col_width(100.0)
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("Device").small().color(MUTED));
                    ui.add(
                        egui::DragValue::new(&mut self.connection.form_rtlsdr.device_index)
                            .range(0..=15)
                            .speed(0.1),
                    );
                    ui.end_row();

                    ui.label(egui::RichText::new("Sample rate").small().color(MUTED));
                    preset_combo_u32(
                        ui,
                        "rtlsdr_sr",
                        &mut self.connection.form_sample_rate,
                        RTLSDR_SAMPLE_RATE_PRESETS,
                        "Hz ",
                        250_000..=3_200_000,
                    );
                    ui.end_row();

                    ui.label(egui::RichText::new("Process IQ").small().color(MUTED));
                    preset_combo_u32(
                        ui,
                        "rtlsdr_proc",
                        &mut self.connection.form_rtlsdr.iq_process_hz,
                        RTLSDR_PROCESS_RATE_PRESETS,
                        "Hz ",
                        0..=3_200_000,
                    );
                    ui.end_row();

                    ui.label(egui::RichText::new("PPM").small().color(MUTED));
                    ui.add(
                        egui::DragValue::new(&mut self.connection.form_rtlsdr.ppm)
                            .range(-200..=200)
                            .speed(0.1),
                    );
                    ui.end_row();

                    ui.label(egui::RichText::new("RTL AGC").small().color(MUTED));
                    ui.toggle_value(&mut self.connection.form_rtlsdr.rtl_agc, "On");
                    ui.end_row();

                    ui.label(egui::RichText::new("Manual gain").small().color(MUTED));
                    ui.toggle_value(&mut self.connection.form_rtlsdr.manual_gain, "On");
                    ui.end_row();

                    if self.connection.form_rtlsdr.manual_gain {
                        ui.label(egui::RichText::new("Tuner gain").small().color(MUTED));
                        ui.add(
                            egui::DragValue::new(&mut self.connection.form_rtlsdr.tuner_gain_db10)
                                .range(0..=500)
                                .speed(0.5)
                                .suffix(" ×0.1 dB"),
                        );
                        ui.end_row();
                    }

                    ui.label(egui::RichText::new("Direct sampling").small().color(MUTED));
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut self.connection.form_rtlsdr.direct_sampling, 0, "Off");
                        ui.selectable_value(&mut self.connection.form_rtlsdr.direct_sampling, 1, "I");
                        ui.selectable_value(&mut self.connection.form_rtlsdr.direct_sampling, 2, "Q");
                    });
                    ui.end_row();

                    ui.label(egui::RichText::new("Offset tune").small().color(MUTED));
                    ui.toggle_value(&mut self.connection.form_rtlsdr.offset_tuning, "On");
                    ui.end_row();

                    ui.label(egui::RichText::new("Bias tee").small().color(MUTED));
                    ui.toggle_value(&mut self.connection.form_rtlsdr.bias_tee, "GPIO DC");
                    ui.end_row();
                });
            section_hint(
                ui,
                "2.048 MHz suits HF with an upconverter. Use direct sampling for 0–28.8 MHz IF \
                 (Q branch often quieter). Lower “Process IQ” to ≤96 kHz for skimmer (reconnect). \
                 Gain / bias / PPM apply live when connected.",
            );
        });
    }

}
