use crate::app::WaterfallApp;
use crate::app::prelude::*;

impl WaterfallApp {

    pub(crate) fn cw_carrier_tools(&mut self, ui: &mut egui::Ui) {
        let bfo = self.radio.cw.bfo_hz.round();
        ui.horizontal(|ui| {
            if ui
                .small_button("Zero-beat (Z)")
                .on_hover_text(format!(
                    "Retune RX so the strongest carrier in view lands on your BFO ({bfo:.0} Hz audio tone); clears RIT"
                ))
                .clicked()
            {
                self.zero_beat();
            }
            toggle(
                ui,
                &mut self.radio.pitch_lock,
                &format!("Lock pitch (L) @ {bfo:.0} Hz"),
            );
        });
    }

    fn cw_filter_design_body(&mut self, ui: &mut egui::Ui) {
        let arch_sel = if self.radio.cw.channel_filter == ChannelFilterKind::LinearFir {
            0
        } else {
            1
        };
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Architecture").small().color(MUTED));
            if let Some(i) = segment_choice(ui, "ch_filter_arch", arch_sel, &["FIR", "IIR 2-pole"]) {
                self.radio.cw.channel_filter = if i == 0 {
                    ChannelFilterKind::LinearFir
                } else {
                    ChannelFilterKind::Iir2Pole
                };
            }
        });
        if self.radio.cw.channel_filter == ChannelFilterKind::LinearFir {
            let shape_sel = match self.radio.cw.window {
                WindowKind::Gaussian => 0,
                WindowKind::RaisedCosine => 1,
                WindowKind::Blackman => 2,
                WindowKind::Kaiser => 3,
            };
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Window").small().color(MUTED));
                if let Some(i) =
                    segment_choice(ui, "ch_filter_win", shape_sel, &["Gauss", "RaisedCos", "Blackman", "Kaiser"])
                {
                    self.radio.cw.window = match i {
                        1 => WindowKind::RaisedCosine,
                        2 => WindowKind::Blackman,
                        3 => WindowKind::Kaiser,
                        _ => WindowKind::Gaussian,
                    };
                }
            });
            if self.radio.cw.window == WindowKind::Kaiser {
                scroll_slider_f32(
                    ui,
                    &mut self.radio.cw.kaiser_beta,
                    MIN_KAISER_BETA..=MAX_KAISER_BETA,
                    "Kaiser β",
                );
            }
            let flatten_resp =
                ui.checkbox(&mut self.radio.cw.passband_flatten, "Flatten passband (inv-sinc)");
            attach_rich_tooltip(
                &flatten_resp,
                Some("Flatten passband"),
                &[
                    ("Inv-sinc lift", ACCENT),
                    (
                        "Lifts upstream boxcar/CIC droop (N≈7). Off by default — enable if the tone sounds dull at band edges.",
                        MUTED,
                    ),
                ],
            );
        }
        let economy = ui.checkbox(
            &mut self.radio.cw.economy_filter,
            "Economy filter (2-pole IIR)",
        );
        attach_rich_tooltip(
            &economy,
            Some("Economy filter"),
            &[
                ("Lower CPU", ACCENT),
                (
                    "Overrides architecture with a 2-pole IIR channel filter. \
                     Steeper skirts but may ring on fast CW.",
                    MUTED,
                ),
            ],
        );
    }

    pub(crate) fn cw_demod_card(&mut self, ui: &mut egui::Ui) {
        collapsible_section(
            ui,
            "cw-demod",
            "CW demod",
            Some(&[
                ("Channel filter", ACCENT),
                (
                    "Complex IQ filter before demod — rejects adjacent signals while the carrier is still recoverable.",
                    MUTED,
                ),
                ("Plot", ACCENT),
                (
                    "Ctrl+scroll: BW · click/drag = tune · Ctrl+drag cyan = RIT · Ctrl+edges = −3 dB BW · Ctrl+purple notches.",
                    MUTED,
                ),
            ]),
            true,
            |ui| {
                popup_section(
                    ui,
                    "Tone & carrier",
                    Some("Sidetone pitch and carrier alignment"),
                    |ui| {
                        ui.label(egui::RichText::new("BFO presets").small().color(MUTED));
                        preset_segment_f32(ui, "bfo_presets", &mut self.radio.cw.bfo_hz, &BFO_PRESETS, 0.5);
                        scroll_slider_f32_step(
                            ui,
                            &mut self.radio.cw.bfo_hz,
                            300.0..=1_200.0,
                            "BFO tone",
                            10.0,
                        );
                        self.cw_carrier_tools(ui);
                    },
                );

                popup_section(
                    ui,
                    "Channel filter",
                    Some("Passband width before demodulation"),
                    |ui| {
                        let wide_sel = usize::from(self.skimmer_ui.filter_wide);
                        ui.label(egui::RichText::new("Range").small().color(MUTED));
                        if let Some(i) = segment_choice(ui, "filter_passband", wide_sel, &["CW", "Wide"]) {
                            let was_wide = self.skimmer_ui.filter_wide;
                            self.skimmer_ui.filter_wide = i == 1;
                            if !self.skimmer_ui.filter_wide {
                                self.radio.cw.full_demod = true;
                            }
                            if self.skimmer_ui.filter_wide && !was_wide
                                && self.radio.cw.passband_hz < CW_PASSBAND_NARROW_MAX_HZ
                            {
                                self.radio.cw.passband_hz = CW_PASSBAND_NARROW_MAX_HZ;
                            }
                        }
                        let bw_max = self.passband_max_hz();
                        let bw_min = if self.skimmer_ui.filter_wide {
                            CW_PASSBAND_NARROW_MAX_HZ
                        } else {
                            CW_PASSBAND_MIN_HZ
                        };
                        if self.radio.cw.passband_hz > bw_max {
                            self.radio.cw.passband_hz = bw_max;
                        } else if self.radio.cw.passband_hz < bw_min {
                            self.radio.cw.passband_hz = bw_min;
                        }
                        let bw_presets: Vec<(&str, f32)> = FILTER_PRESETS
                            .iter()
                            .copied()
                            .filter(|(_, hz)| *hz >= bw_min && *hz <= bw_max)
                            .collect();
                        ui.label(egui::RichText::new("BW presets").small().color(MUTED));
                        preset_segment_f32(
                            ui,
                            "bw_presets",
                            &mut self.radio.cw.passband_hz,
                            &bw_presets,
                            0.5,
                        );
                        scroll_slider_log_f32(
                            ui,
                            &mut self.radio.cw.passband_hz,
                            bw_min..=bw_max,
                            "Channel filter",
                        );
                        let audio_rate =
                            hfsdr::audio_sample_rate(self.radio.sample_rate, self.radio.cw.decimation);
                        let delay_note = if self.radio.cw.channel_filter == ChannelFilterKind::LinearFir {
                            let delay_ms = channel_group_delay_ms(audio_rate, self.radio.cw.passband_hz);
                            let shape_hint = match self.radio.cw.window {
                                WindowKind::Gaussian => {
                                    " · Blackman/Kaiser reject skirt noise better"
                                }
                                WindowKind::RaisedCosine => {
                                    " · Blackman is steeper on adjacent carriers"
                                }
                                _ => "",
                            };
                            format!("Filter delay ~{delay_ms:.0} ms (linear-phase FIR){shape_hint}")
                        } else {
                            "IIR 2-pole — minimal delay, non-linear phase (may ring)".to_string()
                        };
                        ui.label(egui::RichText::new(delay_note).small().color(MUTED));

                        let filter_advanced = self.radio.cw.channel_filter != ChannelFilterKind::LinearFir
                            || self.radio.cw.economy_filter
                            || self.radio.cw.window != WindowKind::RaisedCosine
                            || self.radio.cw.passband_flatten
                            || self.radio.cw.window == WindowKind::Kaiser;
                        egui::CollapsingHeader::new(egui::RichText::new("Filter design").small().color(MUTED))
                            .id_salt("cw_filter_design")
                            .default_open(filter_advanced)
                            .show(ui, |ui| {
                                self.cw_filter_design_body(ui);
                            });
                    },
                );

                popup_section(ui, "Level (AGC)", Some("IQ envelope gain before demod"), |ui| {
                    self.agc_controls(ui);
                });
            },
        );
    }

    pub(crate) fn agc_controls(&mut self, ui: &mut egui::Ui) {
        stage_toggle(
            ui,
            &mut self.radio.cw.agc.enabled,
            "AGC",
            Some("IQ envelope gain riding"),
            Some("A"),
            None,
        );
        if self.radio.cw.agc.enabled {
            let mode_sel = match self.radio.cw.agc_mode {
                AgcMode::Envelope => 0,
                AgcMode::Hang => 1,
                AgcMode::DualLoop => 2,
            };
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Mode").small().color(MUTED));
                if let Some(i) = segment_choice(ui, "agc_mode", mode_sel, &["Envelope", "Hang", "Dual-loop"]) {
                    self.radio.cw.agc_mode = match i {
                        1 => AgcMode::Hang,
                        2 => AgcMode::DualLoop,
                        _ => AgcMode::Envelope,
                    };
                }
            });
            let mode_hint = match self.radio.cw.agc_mode {
                AgcMode::Envelope => "Symmetric attack/decay — general-purpose level riding",
                AgcMode::Hang => "Fast gain reduction, slow recovery — quieter between dits",
                AgcMode::DualLoop => "Peak + floor trackers — resists neighbour-signal pumping",
            };
            ui.label(egui::RichText::new(mode_hint).small().color(MUTED));
            scroll_slider_f32(ui, &mut self.radio.cw.agc.attack_ms, 1.0..=20.0, "Attack ms");
            scroll_slider_f32(ui, &mut self.radio.cw.agc.decay_ms, 20.0..=600.0, "Decay ms");
            scroll_slider_f32(ui, &mut self.radio.cw.agc.target, 0.05..=0.6, "Target");
        } else {
            scroll_slider_f32(ui, &mut self.radio.cw.agc.manual_gain, 0.1..=16.0, "Manual gain");
        }
    }
}
