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
        if !hfsdr::native_sdr::soapy_available() {
            let msg = "libSoapySDR not loaded";
            self.connection.form.soapy_enumerate_error = Some(msg.into());
            log::warn(msg);
            return;
        }
        let drivers = hfsdr::soapy::available_driver_keys();
        if !driver.is_empty() && !drivers.contains(&driver) {
            log::warn(format!(
                "SoapySDR: driver filter '{driver}' has no installed module (available: {})",
                if drivers.is_empty() {
                    "none".into()
                } else {
                    drivers.join(", ")
                }
            ));
        }
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
            let hint = hfsdr::soapy::enumeration_hint(&driver);
            self.connection.form.soapy_enumerate_error = Some(if hint.is_empty() {
                if driver.is_empty() {
                    "No SoapySDR devices found".into()
                } else {
                    format!("No devices for driver '{driver}'")
                }
            } else {
                hint.clone()
            });
            log::warn(if hint.is_empty() {
                if driver.is_empty() {
                    "SoapySDR: no devices found".to_string()
                } else {
                    format!("SoapySDR: no devices for driver '{driver}'")
                }
            } else {
                hint
            });
        } else {
            log::info(format!(
                "SoapySDR: {} device(s) for driver filter '{}'",
                self.connection.form.soapy_device_labels.len(),
                if driver.is_empty() { "all" } else { &driver }
            ));
            if self.connection.form.soapy_device_index
                >= self.connection.form.soapy_device_labels.len()
            {
                self.connection.form.soapy_device_index = 0;
            }
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
        self.probe_soapy_device_rates();
    }

    #[cfg(feature = "soapy")]
    fn probe_soapy_device_rates(&mut self) {
        let args = self.connection.form.soapy.device_args.trim();
        if args.is_empty() {
            self.connection.form.soapy_device_sample_rates.clear();
            return;
        }
        match hfsdr::soapy::probe_sample_rates(args) {
            Ok(rates) if !rates.is_empty() => {
                self.connection.form.soapy_device_sample_rates = rates.clone();
                self.connection.form.sample_rate = hfsdr::soapy::snap_sample_rate(
                    self.connection.form.sample_rate,
                    &self.connection.form.soapy_device_sample_rates,
                );
                log::info(format!(
                    "SoapySDR: {} sample rate(s) from device — using {} Hz",
                    rates.len(),
                    self.connection.form.sample_rate
                ));
            }
            Ok(_) => {
                self.connection.form.soapy_device_sample_rates.clear();
                log::warn(format!(
                    "SoapySDR: device opened but reported no RX sample rates ({args})"
                ));
            }
            Err(e) => {
                self.connection.form.soapy_device_sample_rates.clear();
                log::warn(format!(
                    "SoapySDR: could not probe sample rates for {args}: {e} ({})",
                    hfsdr::soapy::last_error()
                ));
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
                    let args_resp = ui.add(
                        egui::TextEdit::singleline(&mut self.connection.form.soapy.device_args)
                            .hint_text("driver=rtlsdr,serial=…")
                            .desired_width(ui.available_width())
                            .clip_text(true),
                    );
                    if args_resp.lost_focus() {
                        self.probe_soapy_device_rates();
                    }
                    ui.end_row();

                    ui.label(egui::RichText::new("Sample rate").small().color(MUTED));
                    let device_rates = self.connection.form.soapy_device_sample_rates.clone();
                    if device_rates.is_empty() {
                        preset_combo_u32(
                            ui,
                            "soapy_sr",
                            &mut self.connection.form.sample_rate,
                            SOAPY_SAMPLE_RATE_PRESETS,
                            "Hz ",
                            250_000..=32_000_000,
                        );
                    } else {
                        let labels: Vec<String> = device_rates
                            .iter()
                            .map(|&hz| hfsdr::soapy::format_sample_rate(hz))
                            .collect();
                        let presets: Vec<(&str, u32)> = labels
                            .iter()
                            .zip(device_rates.iter())
                            .map(|(label, &hz)| (label.as_str(), hz))
                            .collect();
                        let min = *device_rates.first().unwrap_or(&250_000);
                        let max = *device_rates.last().unwrap_or(&32_000_000);
                        preset_combo_u32(
                            ui,
                            "soapy_sr_dev",
                            &mut self.connection.form.sample_rate,
                            &presets,
                            "Hz ",
                            min..=max,
                        );
                    }
                    ui.end_row();
                    if !device_rates.is_empty() {
                        ui.label("");
                        ui.label(
                            egui::RichText::new(format!(
                                "{} rate(s) from device",
                                device_rates.len()
                            ))
                            .small()
                            .color(MUTED),
                        );
                        ui.end_row();
                    }

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
                 Supported sample rates come from the device. Leave Process IQ at “Native” \
                 unless you need lower CPU — Soapy already delivers IQ in bursts.",
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
