use crate::app::WaterfallApp;
use crate::app::prelude::*;

impl WaterfallApp {

    pub(crate) fn hardware_rf_controls(&mut self, ui: &mut egui::Ui, live: bool) {
        match self.connection.form_kind {
            SourceKind::Kiwi => self.kiwi_rf_controls(ui, live),
            #[cfg(feature = "airspy")]
            SourceKind::Airspy => self.airspy_rf_controls(ui, live),
            #[cfg(feature = "rtlsdr")]
            SourceKind::RtlSdr => self.rtlsdr_rf_controls(ui, live),
            #[cfg(feature = "qmx")]
            SourceKind::Qmx => self.qmx_rf_controls(ui, live),
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
                    "When on, Kiwi runs its own SND AGC — the RF gain slider has no effect on IQ.",
                    MUTED,
                ),
                ("Dual AGC", OK),
                (
                    "Turn off for manual RF gain (Yaesu-style). Software IQ AGC is separate.",
                    MUTED,
                ),
            ]),
        ) {
            self.connection.form_kiwi.rf_agc_on = self.radio.agc_rf_on;
            self.sync_kiwi_rf_now();
        }
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("RF gain").small().color(MUTED));
            let mut gain_db = man_gain_db_below_max(self.connection.form_kiwi.man_gain);
            let resp = ui.add(
                egui::Slider::new(&mut gain_db, -100..=0)
                    .suffix(" dB")
                    .clamping(egui::SliderClamping::Always),
            );
            if resp.changed() {
                self.connection.form_kiwi.man_gain = man_gain_from_db_below_max(gain_db);
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
                if let Some(hw) = self.stats.hw_rf_gain {
                    if hw == self.connection.form_kiwi.man_gain {
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
                Some("RF gain"),
                &[
                    ("Scale", ACCENT),
                    (
                        "0 dB = full gain (Kiwi manGain 100). Each step is ~1 dB; −50 dB is the old Kiwi default.",
                        MUTED,
                    ),
                    ("Kiwi RF AGC off", OK),
                    (
                        "Manual gain applies only with Kiwi RF AGC off — unlike a Yaesu, Kiwi IQ ignores manGain while AGC is on.",
                        MUTED,
                    ),
                    ("Yaesu analogy", MUTED),
                    (
                        "Start at 0 dB (max) and reduce gain if the band is hot — same idea as RF GAIN fully clockwise.",
                        MUTED,
                    ),
                ],
            );
        });
        if !live || self.stats.kiwi_has_rf_attn {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Attenuator").small().color(MUTED));
                let attn_live = live && self.stats.kiwi_has_rf_attn;
                ui.add_enabled_ui(attn_live || !live, |ui| {
                    ui.add(
                        egui::Slider::new(&mut self.connection.form_kiwi.rf_attn_db, 0.0..=31.5)
                            .suffix(" dB")
                            .fixed_decimals(1),
                    );
                });
                if live && !self.stats.kiwi_has_rf_attn {
                    ui.label(
                        egui::RichText::new("(not on this Kiwi)")
                            .small()
                            .color(MUTED),
                    );
                } else if live {
                    ui.label(
                        egui::RichText::new(format!(
                            "hw {:.1} dB",
                            self.stats.kiwi_rf_attn_db
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
            ui.label(egui::RichText::new("RF gain").small().color(MUTED));
            ui.add(
                egui::Slider::new(&mut self.connection.form_qmx.rf_gain_db, 0..=99)
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
            &mut self.connection.form_rtlsdr.rtl_agc,
            "RTL2832 AGC",
            Some("Internal digital AGC in the RTL2832"),
            None,
            None,
        );
        stage_toggle(
            ui,
            &mut self.connection.form_rtlsdr.manual_gain,
            "Manual tuner gain",
            Some("Fixed RF gain from the tuner IC"),
            None,
            None,
        );
        if self.connection.form_rtlsdr.manual_gain {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Gain").small().color(MUTED));
                ui.add(
                    egui::DragValue::new(&mut self.connection.form_rtlsdr.tuner_gain_db10)
                        .range(0..=500)
                        .speed(0.5)
                        .suffix(" ×0.1 dB"),
                );
            });
        }
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("PPM").small().color(MUTED));
            ui.add(
                egui::DragValue::new(&mut self.connection.form_rtlsdr.ppm)
                    .range(-200..=200)
                    .speed(0.1),
            );
        });
        stage_toggle(
            ui,
            &mut self.connection.form_rtlsdr.bias_tee,
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
            &mut self.connection.form_airspy.hf_lna,
            "Preamp (+6 dB LNA)",
            Some("Enable for passive loop/wire antennas; off for max dynamic range"),
            None,
            None,
        );
        stage_toggle(
            ui,
            &mut self.connection.form_airspy.hf_agc,
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
        if self.connection.form_airspy.hf_agc {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("AGC threshold").small().color(MUTED));
                ui.selectable_value(
                    &mut self.connection.form_airspy.hf_agc_threshold_high,
                    false,
                    "Low",
                );
                ui.selectable_value(
                    &mut self.connection.form_airspy.hf_agc_threshold_high,
                    true,
                    "High",
                );
            });
        }
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Attenuator").small().color(MUTED));
            ui.add_enabled(
                !self.connection.form_airspy.hf_agc,
                egui::Slider::new(&mut self.connection.form_airspy.hf_att, 0..=8)
                    .suffix(" ×6 dB"),
            );
        });
        stage_toggle(
            ui,
            &mut self.connection.form_airspy.bias_tee,
            "Bias tee",
            Some("DC on antenna port for active preamps/upconverters"),
            None,
            None,
        );
        ui.collapsing("Frontend options (Discovery / Ranger)", |ui| {
            ui.toggle_value(
                &mut self.connection.form_airspy.frontend_optimize_band_iii,
                "Optimize VHF Band III",
            );
            ui.toggle_value(
                &mut self.connection.form_airspy.frontend_optimize_pll_boundary,
                "Optimize PLL integer boundary",
            );
        });
    }

}
