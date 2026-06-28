use crate::app::WaterfallApp;
use crate::app::prelude::*;

impl WaterfallApp {

    /// Software RF gain — scales IQ for the S-meter, waterfall, and demod chain.
    pub(crate) fn software_rf_gain_control(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("RF gain").color(ACCENT));
            let resp = ui.add(
                egui::Slider::new(&mut self.radio.rf_gain_db, -20.0..=60.0)
                    .suffix(" dB")
                    .fixed_decimals(0)
                    .clamping(egui::SliderClamping::Always),
            );
            if resp.changed() {
                self.lock_display_levels_for_rf_tuning();
            }
            if ui
                .small_button("0")
                .on_hover_text("Reset RF gain to 0 dB")
                .clicked()
            {
                self.radio.rf_gain_db = 0.0;
                self.lock_display_levels_for_rf_tuning();
            }
            attach_rich_tooltip(
                &resp,
                Some("RF gain"),
                &[
                    ("Level", ACCENT),
                    (
                        "Raises or lowers signal on the S-meter, waterfall, and into the demod chain.",
                        MUTED,
                    ),
                    ("Audio volume", OK),
                    (
                        "Listening volume stays steady while the meter still responds to this knob.",
                        MUTED,
                    ),
                    ("Hardware", MUTED),
                    (
                        "Use the hardware controls below if the front end is overloaded.",
                        MUTED,
                    ),
                ],
            );
        });
    }

    pub(crate) fn hardware_rf_controls(&mut self, ui: &mut egui::Ui, live: bool) {
        match self.connection.form.kind {
            SourceKind::Kiwi => self.kiwi_rf_controls(ui, live),
            #[cfg(feature = "airspy")]
            SourceKind::Airspy => self.airspy_rf_controls(ui, live),
            #[cfg(feature = "rtlsdr")]
            SourceKind::RtlSdr => self.rtlsdr_rf_controls(ui, live),
            #[cfg(feature = "qmx")]
            SourceKind::Qmx => self.qmx_rf_controls(ui, live),
            #[cfg(feature = "soapy")]
            SourceKind::Soapy => self.soapy_rf_controls(ui, live),
        }
    }

    pub(crate) fn kiwi_rf_controls(&mut self, ui: &mut egui::Ui, live: bool) {
        if stage_toggle(
            ui,
            &mut self.radio.agc_rf_on,
            "Kiwi RF AGC",
            Some("Hardware RF AGC on the Kiwi (CAT agc=)"),
            None,
            Some(&[
                ("Hardware loop", ACCENT),
                (
                    "Kiwi normalizes IQ in hardware when on — manGain is then applied in software here.",
                    MUTED,
                ),
                ("Dual AGC", OK),
                (
                    "Software IQ AGC holds AF steady; RF gain + manGain still move the S-meter and waterfall.",
                    MUTED,
                ),
            ]),
        ) {
            self.connection.form.kiwi.rf_agc_on = self.radio.agc_rf_on;
            self.sync_kiwi_rf_now();
        }
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("HW gain (manGain)").small().color(MUTED));
            let mut gain_db = man_gain_db_below_max(self.connection.form.kiwi.man_gain);
            let resp = ui.add(
                egui::Slider::new(&mut gain_db, -100..=0)
                    .suffix(" dB")
                    .clamping(egui::SliderClamping::Always),
            );
            if resp.changed() {
                self.connection.form.kiwi.man_gain = man_gain_from_db_below_max(gain_db);
                self.sync_kiwi_rf_now();
            }
            if !self.radio.agc_rf_on {
                ui.label(
                    egui::RichText::new("max")
                        .small()
                        .color(if gain_db == 0 { OK } else { MUTED }),
                );
            }
            if live {
                if let Some(hw) = self.engine_ui.stats.hw_rf_gain {
                    if hw == self.connection.form.kiwi.man_gain {
                        ui.label(
                            egui::RichText::new("sent")
                                .small()
                                .color(OK),
                        );
                    }
                }
            }
            attach_rich_tooltip(
                &resp,
                Some("HW gain (manGain)"),
                &[
                    ("Scale", ACCENT),
                    (
                        "0 dB = full gain (Kiwi manGain 100). Each step is ~1 dB; −50 dB is the old Kiwi default.",
                        MUTED,
                    ),
                    ("Kiwi RF AGC on", OK),
                    (
                        "Kiwi firmware ignores manGain while AGC is on — we apply the same dB in software so the S-meter and waterfall still move.",
                        MUTED,
                    ),
                    ("Kiwi RF AGC off", OK),
                    (
                        "Gain is applied in Kiwi hardware (CAT manGain). Stacks with the RF gain slider above.",
                        MUTED,
                    ),
                ],
            );
        });
        if !live || self.engine_ui.stats.kiwi_has_rf_attn {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Attenuator").small().color(MUTED));
                let attn_live = live && self.engine_ui.stats.kiwi_has_rf_attn;
                ui.add_enabled_ui(attn_live || !live, |ui| {
                    ui.add(
                        egui::Slider::new(&mut self.connection.form.kiwi.rf_attn_db, 0.0..=31.5)
                            .suffix(" dB")
                            .fixed_decimals(1),
                    );
                });
                if live && !self.engine_ui.stats.kiwi_has_rf_attn {
                    ui.label(
                        egui::RichText::new("(not on this Kiwi)")
                            .small()
                            .color(MUTED),
                    );
                } else if live {
                    ui.label(
                        egui::RichText::new(format!(
                            "hw {:.1} dB",
                            self.engine_ui.stats.kiwi_rf_attn_db
                        ))
                        .small()
                        .color(MUTED),
                    );
                }
            });
        }
    }

    #[cfg(feature = "qmx")]
    pub(crate) fn qmx_rf_controls(&mut self, ui: &mut egui::Ui, live: bool) {
        let _ = live;
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("HW gain").small().color(MUTED));
            ui.add(
                egui::Slider::new(&mut self.connection.form.qmx.rf_gain_db, 0..=99)
                    .suffix(" dB")
                    .logarithmic(false),
            );
        });
        if !live {
            ui.label(
                egui::RichText::new("RF gain applies when connected")
                    .small()
                    .color(MUTED),
            );
        }
    }

    #[cfg(feature = "rtlsdr")]
    pub(crate) fn rtlsdr_rf_controls(&mut self, ui: &mut egui::Ui, live: bool) {
        let _ = live;
        stage_toggle(
            ui,
            &mut self.connection.form.rtlsdr.rtl_agc,
            "RTL2832 AGC",
            Some("Internal digital AGC in the RTL2832"),
            None,
            None,
        );
        stage_toggle(
            ui,
            &mut self.connection.form.rtlsdr.manual_gain,
            "Manual tuner gain",
            Some("Fixed RF gain from the tuner IC"),
            None,
            None,
        );
        if self.connection.form.rtlsdr.manual_gain {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Gain").small().color(MUTED));
                ui.add(
                    egui::DragValue::new(&mut self.connection.form.rtlsdr.tuner_gain_db10)
                        .range(0..=500)
                        .speed(0.5)
                        .suffix(" ×0.1 dB"),
                );
            });
        }
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("PPM").small().color(MUTED));
            ui.add(
                egui::DragValue::new(&mut self.connection.form.rtlsdr.ppm)
                    .range(-200..=200)
                    .speed(0.1),
            );
        });
        stage_toggle(
            ui,
            &mut self.connection.form.rtlsdr.bias_tee,
            "Bias tee",
            Some("GPIO bias for active antennas / upconverters"),
            None,
            None,
        );
    }

    #[cfg(feature = "airspy")]
    pub(crate) fn airspy_rf_controls(&mut self, ui: &mut egui::Ui, live: bool) {
        let _ = live;
        stage_toggle(
            ui,
            &mut self.connection.form.airspy.hf_lna,
            "Preamp (+6 dB LNA)",
            Some("Enable for passive loop/wire antennas; off for max dynamic range"),
            None,
            None,
        );
        stage_toggle(
            ui,
            &mut self.connection.form.airspy.hf_agc,
            "HF AGC",
            Some("Hardware AGC on the Airspy front end"),
            None,
            Some(&[
                ("HF AGC on", ACCENT),
                (
                    "Controls front-end gain — turn AGC off to use the 0–48 dB attenuator.",
                    MUTED,
                ),
            ]),
        );
        if self.connection.form.airspy.hf_agc {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("AGC threshold").small().color(MUTED));
                ui.selectable_value(
                    &mut self.connection.form.airspy.hf_agc_threshold_high,
                    false,
                    "Low",
                );
                ui.selectable_value(
                    &mut self.connection.form.airspy.hf_agc_threshold_high,
                    true,
                    "High",
                );
            });
        }
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Attenuator").small().color(MUTED));
            ui.add_enabled(
                !self.connection.form.airspy.hf_agc,
                egui::Slider::new(&mut self.connection.form.airspy.hf_att, 0..=8)
                    .suffix(" ×6 dB"),
            );
        });
        stage_toggle(
            ui,
            &mut self.connection.form.airspy.bias_tee,
            "Bias tee",
            Some("DC on antenna port for active preamps/upconverters"),
            None,
            None,
        );
        ui.collapsing("Frontend options (Discovery / Ranger)", |ui| {
            ui.toggle_value(
                &mut self.connection.form.airspy.frontend_optimize_band_iii,
                "Optimize VHF Band III",
            );
            ui.toggle_value(
                &mut self.connection.form.airspy.frontend_optimize_pll_boundary,
                "Optimize PLL integer boundary",
            );
        });
    }

    #[cfg(feature = "soapy")]
    pub(crate) fn soapy_rf_controls(&mut self, ui: &mut egui::Ui, live: bool) {
        let _ = live;
        if stage_toggle(
            ui,
            &mut self.connection.form.soapy.agc,
            "Soapy AGC",
            Some("Hardware automatic gain on the Soapy device"),
            None,
            None,
        ) {
            self.sync_soapy_rf_now();
        }
        if !self.connection.form.soapy.agc {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("HW gain").small().color(MUTED));
                let resp = ui.add(
                    egui::Slider::new(&mut self.connection.form.soapy.gain_db, 0.0..=80.0)
                        .suffix(" dB")
                        .fixed_decimals(1),
                );
                if resp.changed() {
                    self.sync_soapy_rf_now();
                }
            });
        }
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Antenna").small().color(MUTED));
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.connection.form.soapy.antenna)
                    .hint_text("RX1, HF, …"),
            );
            if resp.lost_focus() && resp.changed() {
                self.sync_soapy_rf_now();
            }
        });
        if !live {
            ui.label(
                egui::RichText::new("Gain and antenna apply when connected")
                    .small()
                    .color(MUTED),
            );
        }
    }

}
