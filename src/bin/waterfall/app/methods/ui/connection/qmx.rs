use crate::app::WaterfallApp;
use crate::app::prelude::*;

impl WaterfallApp {

    #[cfg(feature = "qmx")]
    pub(crate) fn connection_qmx_section(&mut self, ui: &mut egui::Ui) {
        let serial_ports = hfsdr::qmx::list_serial_ports();
        let audio_inputs = hfsdr::qmx::list_input_devices();
        popup_section(ui, "QMX / QMX+", None, |ui| {
            egui::Grid::new("connect_qmx_grid")
                .num_columns(2)
                .spacing([8.0, 4.0])
                .min_col_width(100.0)
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("CAT port").small().color(MUTED));
                    egui::ComboBox::from_id_salt("qmx_serial")
                        .selected_text(if self.connection.form_qmx.serial_port.is_empty() {
                            "(first available)".to_string()
                        } else {
                            self.connection.form_qmx.serial_port.clone()
                        })
                        .width(ui.available_width())
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_label(self.connection.form_qmx.serial_port.is_empty(), "(first available)")
                                .clicked()
                            {
                                self.connection.form_qmx.serial_port.clear();
                            }
                            for port in &serial_ports {
                                if ui
                                    .selectable_label(
                                        self.connection.form_qmx.serial_port == *port,
                                        port,
                                    )
                                    .clicked()
                                {
                                    self.connection.form_qmx.serial_port = port.clone();
                                }
                            }
                        });
                    ui.end_row();

                    ui.label(egui::RichText::new("IQ audio in").small().color(MUTED));
                    egui::ComboBox::from_id_salt("qmx_audio")
                        .selected_text(if self.connection.form_qmx.audio_device.is_empty() {
                            "(auto-detect QMX)".to_string()
                        } else {
                            self.connection.form_qmx.audio_device.clone()
                        })
                        .width(ui.available_width())
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_label(self.connection.form_qmx.audio_device.is_empty(), "(auto-detect QMX)")
                                .clicked()
                            {
                                self.connection.form_qmx.audio_device.clear();
                            }
                            for dev in &audio_inputs {
                                if ui
                                    .selectable_label(
                                        self.connection.form_qmx.audio_device == *dev,
                                        dev,
                                    )
                                    .clicked()
                                {
                                    self.connection.form_qmx.audio_device = dev.clone();
                                }
                            }
                        });
                    ui.end_row();

                    ui.label(egui::RichText::new("Process IQ").small().color(MUTED));
                    preset_combo_u32(
                        ui,
                        "qmx_proc",
                        &mut self.connection.form_qmx.iq_process_hz,
                        QMX_PROCESS_RATE_PRESETS,
                        "Hz ",
                        0..=48_000,
                    );
                    ui.end_row();

                    ui.label(egui::RichText::new("IF offset").small().color(MUTED));
                    ui.add(
                        egui::DragValue::new(&mut self.connection.form_qmx.if_offset_hz)
                            .range(0..=50_000)
                            .suffix(" Hz"),
                    );
                    ui.end_row();

                    ui.label(egui::RichText::new("RF gain").small().color(MUTED));
                    ui.add(
                        egui::DragValue::new(&mut self.connection.form_qmx.rf_gain_db)
                            .range(0..=99)
                            .suffix(" dB"),
                    );
                    ui.end_row();

                    ui.label(egui::RichText::new("CAT timeout").small().color(MUTED));
                    ui.toggle_value(
                        &mut self.connection.form_qmx.disable_cat_timeout,
                        "Disable (stay in RX)",
                    );
                    ui.end_row();

                    ui.label(egui::RichText::new("CW mode").small().color(MUTED));
                    ui.toggle_value(&mut self.connection.form_qmx.force_cw_mode, "Set at connect");
                    ui.end_row();
                });
            section_hint(
                ui,
                "IQ is 48 kHz stereo USB audio (I=left, Q=right). CAT enables IQ mode (Q9) \
                 and tunes VFO A (FA). The 12 kHz IF offset is applied automatically. \
                 RF gain applies live when connected; port/audio choices need reconnect.",
            );
        });
    }

}
