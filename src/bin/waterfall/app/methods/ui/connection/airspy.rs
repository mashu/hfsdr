// `ui/connection/airspy` — Airspy HF+ connection settings.

    #[cfg(feature = "airspy")]
    fn connection_airspy_section(&mut self, ui: &mut egui::Ui) {
        popup_section(ui, "Airspy HF+", None, |ui| {
            egui::Grid::new("connect_airspy_grid")
                .num_columns(2)
                .spacing([8.0, 4.0])
                .min_col_width(100.0)
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("Sample rate").small().color(MUTED));
                    preset_combo_u32(
                        ui,
                        "airspy_sr",
                        &mut self.form_sample_rate,
                        AIRSPY_SAMPLE_RATE_PRESETS,
                        "Hz ",
                        3_000..=768_000,
                    );
                    ui.end_row();

                    ui.label(egui::RichText::new("Process IQ").small().color(MUTED));
                    preset_combo_u32(
                        ui,
                        "airspy_proc",
                        &mut self.form_airspy.iq_process_hz,
                        AIRSPY_PROCESS_RATE_PRESETS,
                        "Hz ",
                        0..=768_000,
                    );
                    ui.end_row();

                    ui.label(egui::RichText::new("HF AGC").small().color(MUTED));
                    ui.toggle_value(&mut self.form_airspy.hf_agc, "On");
                    ui.end_row();

                    ui.label(egui::RichText::new("AGC threshold").small().color(MUTED));
                    ui.horizontal(|ui| {
                        ui.selectable_value(
                            &mut self.form_airspy.hf_agc_threshold_high,
                            false,
                            "Low",
                        );
                        ui.selectable_value(
                            &mut self.form_airspy.hf_agc_threshold_high,
                            true,
                            "High",
                        );
                    });
                    ui.end_row();

                    ui.label(egui::RichText::new("Attenuator").small().color(MUTED));
                    ui.add(
                        egui::DragValue::new(&mut self.form_airspy.hf_att)
                            .range(0..=8)
                            .suffix(" ×6 dB"),
                    );
                    ui.end_row();

                    ui.label(egui::RichText::new("Preamp").small().color(MUTED));
                    ui.toggle_value(&mut self.form_airspy.hf_lna, "+6 dB LNA (passive ant.)");
                    ui.end_row();

                    ui.label(egui::RichText::new("Bias tee").small().color(MUTED));
                    ui.toggle_value(
                        &mut self.form_airspy.bias_tee,
                        "Antenna DC (active preamp)",
                    );
                    ui.end_row();

                    ui.label(egui::RichText::new("Frontend").small().color(MUTED));
                    ui.vertical(|ui| {
                        ui.toggle_value(
                            &mut self.form_airspy.frontend_optimize_band_iii,
                            "Optimize VHF Band III",
                        );
                        ui.toggle_value(
                            &mut self.form_airspy.frontend_optimize_pll_boundary,
                            "Optimize PLL int. boundary",
                        );
                    });
                    ui.end_row();

                    ui.label(egui::RichText::new("Lib DSP").small().color(MUTED));
                    ui.toggle_value(&mut self.form_airspy.lib_dsp, "IQ correction");
                    ui.end_row();
                });
            section_hint(
                ui,
                "384 kHz is a good CW default. Lower “Process IQ” cuts CPU load (reconnect). \
                 Preamp/Att/AGC apply live when connected. Discovery HF+ band-tracking \
                 preselectors are automatic — no manual filter-bank setting in libairspyhf.",
            );
        });
    }
