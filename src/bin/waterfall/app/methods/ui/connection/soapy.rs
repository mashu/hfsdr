use crate::app::WaterfallApp;
use crate::app::prelude::*;

#[cfg(feature = "soapy")]
fn soapy_kwarg<'a>(args: &'a str, key: &str) -> Option<&'a str> {
    for part in args.split(',') {
        let part = part.trim();
        let (k, v) = part.split_once('=')?;
        if k == key {
            return Some(v);
        }
    }
    None
}

#[cfg(feature = "soapy")]
fn soapy_device_list_label(raw: &str, args: &str) -> String {
    if let Some(label) = soapy_kwarg(args, "label") {
        if let Some(serial) = soapy_kwarg(args, "serial") {
            let serial_short = if serial.len() <= 10 {
                serial.to_string()
            } else {
                format!("…{}", &serial[serial.len().saturating_sub(8)..])
            };
            return format!("{label} · {serial_short}");
        }
        return truncate_middle(label, 44);
    }
    if let Some(driver) = soapy_kwarg(args, "driver") {
        return truncate_middle(&format!("{driver} · {raw}"), 44);
    }
    truncate_middle(raw, 44)
}

impl WaterfallApp {

    #[cfg(feature = "soapy")]
    pub(crate) fn refresh_soapy_devices(&mut self) {
        self.connection.form.soapy_enumerate_error = None;
        let driver = self.connection.form.soapy.driver.clone();
        let devices = hfsdr::soapy::enumerate_devices(&driver);
        self.connection.form.soapy_device_labels.clear();
        self.connection.form.soapy_device_args_list.clear();
        for (label, args) in devices {
            self.connection
                .form
                .soapy_device_labels
                .push(soapy_device_list_label(&label, &args));
            self.connection.form.soapy_device_args_list.push(args);
        }
        if self.connection.form.soapy_device_labels.is_empty() {
            self.connection.form.soapy_enumerate_error = Some(if driver.is_empty() {
                "No SoapySDR devices found".into()
            } else {
                format!("No devices for driver '{driver}'")
            });
        } else if self.connection.form.soapy_device_index
            >= self.connection.form.soapy_device_labels.len()
        {
            self.connection.form.soapy_device_index = 0;
        }
        self.sync_soapy_selection_from_index();
    }

    #[cfg(feature = "soapy")]
    fn sync_soapy_selection_from_index(&mut self) {
        if let Some(args) = self
            .connection
            .form
            .soapy_device_args_list
            .get(self.connection.form.soapy_device_index)
        {
            self.connection.form.soapy.device_args = args.clone();
            if let Some(rest) = args.strip_prefix("driver=") {
                if let Some(driver) = rest.split(',').next() {
                    self.connection.form.soapy.driver = driver.to_string();
                }
            }
        }
    }

    #[cfg(feature = "soapy")]
    pub(crate) fn connection_soapy_section(&mut self, ui: &mut egui::Ui) {
        popup_section(ui, "SoapySDR", None, |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.button("Refresh devices").clicked() {
                    self.refresh_soapy_devices();
                }
                if let Some(err) = &self.connection.form.soapy_enumerate_error {
                    ui.colored_label(WARN, err);
                } else if !self.connection.form.soapy_device_labels.is_empty() {
                    ui.label(
                        egui::RichText::new(format!(
                            "{} found",
                            self.connection.form.soapy_device_labels.len()
                        ))
                        .small()
                        .color(MUTED),
                    );
                }
            });

            egui::Grid::new("connect_soapy_grid")
                .num_columns(2)
                .spacing([8.0, 4.0])
                .min_col_width(100.0)
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("Driver filter").small().color(MUTED));
                    egui::ComboBox::from_id_salt("soapy_driver")
                        .width(ui.available_width())
                        .selected_text(if self.connection.form.soapy.driver.is_empty() {
                            "All drivers"
                        } else {
                            self.connection.form.soapy.driver.as_str()
                        })
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_label(self.connection.form.soapy.driver.is_empty(), "All drivers")
                                .clicked()
                            {
                                self.connection.form.soapy.driver.clear();
                                self.refresh_soapy_devices();
                            }
                            for d in SOAPY_DRIVER_PRESETS {
                                if ui
                                    .selectable_label(self.connection.form.soapy.driver == *d, *d)
                                    .clicked()
                                {
                                    self.connection.form.soapy.driver = (*d).to_string();
                                    self.refresh_soapy_devices();
                                }
                            }
                        });
                    ui.end_row();

                    ui.label(egui::RichText::new("Device").small().color(MUTED));
                    if self.connection.form.soapy_device_labels.is_empty() {
                        ui.label(egui::RichText::new("— refresh to scan —").small().color(MUTED));
                    } else {
                        let prev = self.connection.form.soapy_device_index;
                        let selected = self
                            .connection
                            .form
                            .soapy_device_labels
                            .get(self.connection.form.soapy_device_index)
                            .map(String::as_str)
                            .unwrap_or("Select…");
                        egui::ComboBox::from_id_salt("soapy_device")
                            .width(ui.available_width())
                            .selected_text(selected)
                            .show_ui(ui, |ui| {
                                for (i, label) in self.connection.form.soapy_device_labels.iter().enumerate() {
                                    if ui
                                        .selectable_label(self.connection.form.soapy_device_index == i, label)
                                        .clicked()
                                    {
                                        self.connection.form.soapy_device_index = i;
                                    }
                                }
                                if let Some(args) = self
                                    .connection
                                    .form
                                    .soapy_device_args_list
                                    .get(self.connection.form.soapy_device_index)
                                {
                                    ui.separator();
                                    ui.label(
                                        egui::RichText::new(truncate_middle(args, 72))
                                            .small()
                                            .monospace()
                                            .color(MUTED),
                                    );
                                }
                            });
                        if self.connection.form.soapy_device_index != prev {
                            self.sync_soapy_selection_from_index();
                        }
                    }
                    ui.end_row();

                    ui.label(egui::RichText::new("Device args").small().color(MUTED));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.connection.form.soapy.device_args)
                            .hint_text("driver=rtlsdr,serial=…")
                            .desired_width(ui.available_width())
                            .clip_text(true),
                    );
                    ui.end_row();

                    ui.label(egui::RichText::new("Sample rate").small().color(MUTED));
                    preset_combo_u32(
                        ui,
                        "soapy_sr",
                        &mut self.connection.form.sample_rate,
                        SOAPY_SAMPLE_RATE_PRESETS,
                        "Hz ",
                        250_000..=32_000_000,
                    );
                    ui.end_row();

                    ui.label(egui::RichText::new("Process IQ").small().color(MUTED));
                    preset_combo_u32(
                        ui,
                        "soapy_proc",
                        &mut self.connection.form.soapy.iq_process_hz,
                        SOAPY_PROCESS_RATE_PRESETS,
                        "Hz ",
                        0..=32_000_000,
                    );
                    ui.end_row();

                    ui.label(egui::RichText::new("Antenna").small().color(MUTED));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.connection.form.soapy.antenna)
                            .hint_text("optional — RX1, HF, …")
                            .desired_width(ui.available_width())
                            .clip_text(true),
                    );
                    ui.end_row();

                    ui.label(egui::RichText::new("AGC").small().color(MUTED));
                    ui.toggle_value(&mut self.connection.form.soapy.agc, "Auto gain");
                    ui.end_row();

                    if !self.connection.form.soapy.agc {
                        ui.label(egui::RichText::new("Gain").small().color(MUTED));
                        ui.add(
                            egui::DragValue::new(&mut self.connection.form.soapy.gain_db)
                                .range(0.0..=80.0)
                                .speed(0.5)
                                .suffix(" dB"),
                        );
                        ui.end_row();
                    }
                });
            section_hint(
                ui,
                "SoapySDR wraps many SDR drivers (RTL-SDR, Airspy HF+, HackRF, Pluto, …). \
                 Pick a driver filter, refresh, then choose a device. Advanced users can edit \
                 device args directly. Gain and antenna apply live when connected.",
            );
        });
    }

}

#[cfg(all(test, feature = "soapy"))]
mod tests {
    use super::soapy_device_list_label;

    #[test]
    fn soapy_list_label_shortens_long_args() {
        let args = "driver=airspyhf,label=AirSpy HF+ [dd52978035b42535],serial=dd52978035b42535";
        let label = soapy_device_list_label("AirSpy HF+ [dd52978035b42535]", args);
        assert!(label.contains("AirSpy HF+"));
        assert!(label.contains('·'));
        assert!(label.chars().count() <= 48);
    }
}
