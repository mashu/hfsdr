use crate::app::WaterfallApp;
use crate::app::prelude::*;

impl WaterfallApp {

    pub(crate) fn connection_kiwi_iq_section(&mut self, ui: &mut egui::Ui) {
        popup_section(ui, "Kiwi IQ", None, |ui| {
            egui::Grid::new("connect_kiwi_grid")
                .num_columns(2)
                .spacing([8.0, 4.0])
                .min_col_width(100.0)
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("IQ rate").small().color(MUTED));
                    preset_combo_u32(
                        ui,
                        "kiwi_iq_rate",
                        &mut self.connection.form.kiwi.iq_rate_hz,
                        KIWI_IQ_RATE_PRESETS,
                        "Hz ",
                        4_000..=30_000,
                    );
                    ui.end_row();

                    ui.label(egui::RichText::new("Bandwidth").small().color(MUTED));
                    preset_combo_u32(
                        ui,
                        "kiwi_bw",
                        &mut self.connection.form.kiwi.iq_half_bw_hz,
                        KIWI_BW_PRESETS,
                        "±Hz ",
                        0..=30_000,
                    );
                    ui.end_row();

                    ui.label(egui::RichText::new("Resample").small().color(MUTED));
                    preset_combo_u32(
                        ui,
                        "kiwi_resample",
                        &mut self.connection.form.kiwi.iq_resample_hz,
                        KIWI_RESAMPLE_PRESETS,
                        "Hz ",
                        0..=48_000,
                    );
                    ui.end_row();

                    ui.label(egui::RichText::new("LNB LO").small().color(MUTED));
                    preset_combo_f64(
                        ui,
                        "kiwi_lo",
                        &mut self.connection.form.kiwi.freq_offset_khz,
                        KIWI_LO_PRESETS,
                        "kHz ",
                        0.0..=1_000_000.0,
                    );
                    ui.end_row();

                    ui.label(egui::RichText::new("AR out").small().color(MUTED));
                    preset_combo_u32(
                        ui,
                        "kiwi_ar",
                        &mut self.connection.form.kiwi.ar_out_hz,
                        KIWI_AR_OUT_PRESETS,
                        "Hz ",
                        8_000..=192_000,
                    );
                    ui.end_row();

                    ui.label(egui::RichText::new("RF attn").small().color(MUTED));
                    ui.add(
                        egui::DragValue::new(&mut self.connection.form.kiwi.rf_attn_db)
                            .range(0.0..=31.5)
                            .speed(0.1)
                            .suffix(" dB"),
                    );
                    ui.end_row();

                    ui.label(egui::RichText::new("Gen attn").small().color(MUTED));
                    ui.add(
                        egui::DragValue::new(&mut self.connection.form.kiwi.gen_attn)
                            .range(0..=255)
                            .suffix(" (handshake)"),
                    );
                    ui.end_row();
                });
        });
    }

}
