use crate::app::WaterfallApp;
use crate::app::prelude::*;

impl WaterfallApp {

    pub(crate) fn smeter_card(&mut self, ui: &mut egui::Ui) {
        let live = matches!(self.engine_ui.conn_state, ConnState::Streaming);
        section_frame()
            .inner_margin(egui::Margin::symmetric(8, 6))
            .show(ui, |ui| {
            let w = ui.available_width();
            ui.set_min_width(w);
            ui.set_max_width(w);
            section_heading_with_tip(
                ui,
                "S-meter",
                &[
                    ("RF level", ACCENT),
                    (
                        "Pre-software-AGC IQ + Kiwi hardware SND — independent of the IF IQ AGC loop.",
                        MUTED,
                    ),
                    ("IF IQ AGC", ACCENT),
                    (
                        "Software loop that holds AF steady — independent of the S-meter needle.",
                        MUTED,
                    ),
                    ("AF peak", OK),
                    ("Post-AGC audio level; aim near half scale when tuning RF gain.", MUTED),
                ],
            );
            show_dual_agc_loop(
                ui,
                &DualAgcParams {
                    rf_dbm: if live {
                        self.rf_meter_dbm()
                    } else {
                        -127.0
                    },
                    hw_rssi_dbm: if live {
                        self.engine_ui.stats.rssi_dbm
                    } else {
                        None
                    },
                    agc_gain: if live {
                        self.engine_ui.stats.agc_gain
                    } else {
                        1.0
                    },
                    agc_enabled: live && self.radio.cw.agc.enabled,
                    audio_peak: if live {
                        self.engine_ui.stats.audio_peak
                    } else {
                        0.0
                    },
                    streaming: live,
                },
            );
        });
    }



    pub(crate) fn frequency_card(&mut self, ui: &mut egui::Ui) {
        section_card(ui, |ui| {
            section_heading(ui, "Operator");
            ui.label(egui::RichText::new("HF — all amateur bands 160m–10m").small().color(MUTED));
            self.band_preset_buttons(ui, &CW_HF_BAND_PRESETS);
            ui.label(egui::RichText::new("VHF+").small().color(MUTED));
            self.band_preset_buttons(ui, &CW_VHF_BAND_PRESETS);
            ui.horizontal(|ui| {
                let mut vfo_changed = false;
                ui.vertical(|ui| {
                    vfo_changed = vfo_wheel_khz(ui, &mut self.radio.center_khz);
                });
                ui.with_layout(
                    egui::Layout::bottom_up(egui::Align::Min),
                    |ui| {
                        if band_lock_toggle(ui, &mut self.radio.lock_ham_bands) {
                            if self.radio.lock_ham_bands {
                                self.clamp_center_to_ham_bands();
                                vfo_changed = true;
                            }
                        }
                    },
                );
                if vfo_changed {
                    self.clamp_center_to_ham_bands();
                    self.apply_radio_settings();
                }
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                scroll_slider_f32_step(ui, &mut self.radio.rit_hz, -800.0..=800.0, "RIT", 1.0);
                let rit_on = self.radio.rit_hz.abs() > 0.5;
                if ui
                    .add_enabled(
                        rit_on,
                        egui::Button::new("Clear").min_size(egui::vec2(0.0, 0.0)),
                    )
                    .on_hover_text("Listen offset → 0 Hz without moving RX center (\\)")
                    .clicked()
                {
                    self.clear_rit();
                }
            });
        });
    }



    pub(crate) fn rf_front_end_card(&mut self, ui: &mut egui::Ui) {
        let live = matches!(self.engine_ui.conn_state, ConnState::Streaming);
        section_card(ui, |ui| {
            section_heading_with_tip(
                ui,
                "RF front-end",
                &[
                    ("RF gain", ACCENT),
                    (
                        "Yaesu-style software gain — moves the S-meter and waterfall on every radio, even with hardware/RF AGC on.",
                        MUTED,
                    ),
                    ("Hardware front-end", OK),
                    ("Per-radio gain/attenuator/AGC below set what the SDR actually sends.", MUTED),
                ],
            );
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(self.connection_alias())
                        .small()
                        .color(MUTED),
                );
                if !live {
                    ui.label(
                        egui::RichText::new("offline — live on connect")
                            .small()
                            .color(MUTED),
                    );
                }
            });
            self.software_rf_gain_control(ui);
            ui.add_space(4.0);
            ui.label(egui::RichText::new("Hardware front-end").small().color(MUTED));
            self.hardware_rf_controls(ui, live);
        });
    }



    pub(crate) fn receive_chain_card(&mut self, ui: &mut egui::Ui) {
        collapsible_section(
            ui,
            "pipeline",
            "Receive chain",
            Some(&[
                ("Order", ACCENT),
                (
                    "Stages run top-to-bottom. Prefer IQ notches + channel filter before post-demod polish.",
                    MUTED,
                ),
                ("① IQ", OK),
                ("Noise blanker → manual notches (keys 1–4, ±80 Hz).", MUTED),
                ("②–④", OK),
                ("Channel filter + AGC + BFO in CW demod panel (right).", MUTED),
                ("⑤ Audio", ACCENT),
                ("APF, auto-notch, NR — optional post-demod stages.", MUTED),
            ]),
            true,
            |ui| {
            ui.label(egui::RichText::new("① IQ — before demod").small().color(MUTED));
            stage_toggle(
                ui,
                &mut self.radio.cw.noise_blanker.enabled,
                "Noise blanker",
                Some("Wideband IQ impulse blanker"),
                Some("B"),
                Some(&[
                    ("Raw IQ", ACCENT),
                    (
                        "Blank lightning/ignition impulses — must run before the narrow channel filter.",
                        WARN,
                    ),
                ]),
            );
            if self.radio.cw.noise_blanker.enabled {
                scroll_slider_f32(ui, &mut self.radio.cw.noise_blanker.threshold, 2.0..=12.0, "NB threshold");
                let mut width = self.radio.cw.noise_blanker.width as f32;
                scroll_slider_f32(ui, &mut width, 1.0..=30.0, "NB recovery");
                self.radio.cw.noise_blanker.width = width.round() as usize;
            }

            ui.separator();
            self.manual_notches_body(ui);

            ui.separator();
            ui.label(egui::RichText::new("⑤ Audio — after BFO demod (optional)").small().color(MUTED));
            stage_toggle(
                ui,
                &mut self.radio.cw.apf.enabled,
                "Audio peak filter",
                Some("Resonant boost at BFO pitch"),
                Some("P"),
                None,
            );
            if self.radio.cw.apf.enabled {
                scroll_slider_f32(ui, &mut self.radio.cw.apf.width_hz, 40.0..=300.0, "APF width");
                scroll_slider_f32(ui, &mut self.radio.cw.apf.gain, 0.2..=4.0, "APF gain");
            }

            stage_toggle(
                ui,
                &mut self.radio.cw.auto_notch.enabled,
                "Auto-notch",
                Some("Audio LMS with BFO guard"),
                Some("N"),
                Some(&[
                    ("Post-demod", ACCENT),
                    (
                        "Can see your BFO tone and freeze while you copy.",
                        MUTED,
                    ),
                    (
                        "Purple IQ notches above are better for hets — they run before demod.",
                        OK,
                    ),
                ]),
            );
            if self.radio.cw.auto_notch.enabled {
                scroll_slider_f32(ui, &mut self.radio.cw.auto_notch.guard_hz, 60.0..=300.0, "Guard ±Hz");
                scroll_slider_f32(ui, &mut self.radio.cw.auto_notch.rate, 0.002..=0.1, "Adapt rate");
            }

            stage_toggle(
                ui,
                &mut self.radio.cw.noise_reduction.enabled,
                "Noise reduction",
                Some("Light audio LMS polish"),
                Some("R"),
                Some(&[
                    ("Optional polish", ACCENT),
                    (
                        "The IQ channel filter is the real noise remover — NR does not belong before demod.",
                        MUTED,
                    ),
                ]),
            );
            if self.radio.cw.noise_reduction.enabled {
                scroll_slider_f32(ui, &mut self.radio.cw.noise_reduction.level, 0.0..=0.5, "NR level");
            }
        });
    }



    pub(crate) fn manual_notches_body(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let label = ui.label(
                egui::RichText::new("Manual notches — complex IQ")
                    .small()
                    .color(MUTED),
            );
            let hint = ui.label(egui::RichText::new("(?)").small().color(MUTED));
            let tip = &[
                ("Pre-demod", ACCENT),
                (
                    "Removes hets while the carrier is still recoverable. Drag purple markers on the spectrum.",
                    MUTED,
                ),
                ("Keys 1–4", OK),
                ("Toggle notches · new ones land on listen ±80 Hz.", MUTED),
            ];
            attach_rich_tooltip(&label, Some("Manual notches"), tip);
            attach_rich_tooltip(&hint, Some("Manual notches"), tip);
        });
        for idx in 0..MAX_NOTCHES {
            let was_enabled = self.radio.cw.notches[idx].enabled;
            let key = match idx {
                0 => "1",
                1 => "2",
                2 => "3",
                _ => "4",
            };
            stage_toggle(
                ui,
                &mut self.radio.cw.notches[idx].enabled,
                &format!("Manual notch #{}", idx + 1),
                Some("Complex IQ — drag on spectrum"),
                Some(key),
                None,
            );
            if self.radio.cw.notches[idx].enabled && !was_enabled {
                self.arm_manual_notch(idx, None);
            }
            if self.radio.cw.notches[idx].enabled {
                let notch = &mut self.radio.cw.notches[idx];
                let mut offset_hz = notch.offset_hz.hz();
                scroll_slider_f32_step(
                    ui,
                    &mut offset_hz,
                    -5_000.0..=5_000.0,
                    "Offset",
                    1.0,
                );
                notch.offset_hz = ChannelOffsetHz::new(offset_hz);
                scroll_slider_f32_step(ui, &mut notch.width_hz, 10.0..=200.0, "Width", 1.0);
            }
        }
    }


}
