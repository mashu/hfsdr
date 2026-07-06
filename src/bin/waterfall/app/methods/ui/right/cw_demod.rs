use crate::app::WaterfallApp;
use crate::app::prelude::*;

impl WaterfallApp {

    pub(crate) fn cw_carrier_tools(&mut self, ui: &mut egui::Ui) {
        let bfo = self.radio.cw.bfo_hz.round();
        ui.horizontal(|ui| {
            if ui
                .small_button("Zero-beat (Z)")
                .on_hover_text(format!(
                    "Retune RX so the strongest carrier in view lands on your BFO ({bfo:.0} Hz audio tone); clears RIT and SHIFT"
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

    pub(crate) fn cw_demod_card(&mut self, ui: &mut egui::Ui) {
        let simple = self.chrome.cw_simple_ui;
        collapsible_section(
            ui,
            "cw-demod",
            "CW demod",
            Some(if simple {
                &[
                    ("Simple layout", ACCENT),
                    (
                        "BFO, passband width, and AGC essentials. Toggle Simple off in the \
                         status bar for filter design, skimmer, and IQ tools.",
                        MUTED,
                    ),
                    ("Plot", ACCENT),
                    (
                        "Ctrl+scroll: BW · click = tune · drag = walk RX · Ctrl+drag cyan = shift filter.",
                        MUTED,
                    ),
                ]
            } else {
                &[
                    ("Channel filter", ACCENT),
                    (
                        "IQ bandpass before demod — BW sets width; Filter design sets how sharply \
                         skirts reject adjacent carriers (FIR window).",
                        MUTED,
                    ),
                    ("Plot", ACCENT),
                    (
                        "Ctrl+scroll: BW · click = tune · drag = walk RX · Ctrl+drag cyan = shift filter · Ctrl+edges = BW · Ctrl+purple = notches.",
                        MUTED,
                    ),
                ]
            }),
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
                        let band_sideband =
                            cw_sideband_for_center(self.radio.center_khz * 1000.0);
                        let sideband_sel = if self.radio.sideband_auto {
                            0
                        } else if self.radio.cw.sideband == CwSideband::Lower {
                            1
                        } else {
                            2
                        };
                        if let Some(i) = labeled_segment_choice(
                            ui,
                            "cw_sideband",
                            "Demod",
                            sideband_sel,
                            &["Auto", "CW-L", "CW-U"],
                            36.0,
                        ) {
                            match i {
                                1 => {
                                    self.radio.sideband_auto = false;
                                    self.radio.cw.sideband = CwSideband::Lower;
                                }
                                2 => {
                                    self.radio.sideband_auto = false;
                                    self.radio.cw.sideband = CwSideband::Upper;
                                }
                                _ => {
                                    self.radio.sideband_auto = true;
                                    self.sync_sideband_from_band();
                                }
                            }
                        }
                        let band_label = Self::cw_band_for_center(self.radio.center_khz * 1000.0)
                            .map(|b| b.label)
                            .unwrap_or("off-band");
                        let band_sideband_label = match band_sideband {
                            CwSideband::Lower => "CW-L",
                            CwSideband::Upper => "CW-U",
                        };
                        let sideband_hint = if self.radio.sideband_auto {
                            format!(
                                "Band plan ({band_label}): {band_sideband_label} — tune {} the carrier",
                                if band_sideband == CwSideband::Lower {
                                    "above"
                                } else {
                                    "below"
                                }
                            )
                        } else {
                            format!(
                                "Manual {} — tune {} the carrier",
                                match self.radio.cw.sideband {
                                    CwSideband::Lower => "CW-L",
                                    CwSideband::Upper => "CW-U",
                                },
                                if self.radio.cw.sideband == CwSideband::Lower {
                                    "above"
                                } else {
                                    "below"
                                }
                            )
                        };
                        ui.label(egui::RichText::new(sideband_hint).small().color(MUTED));
                        self.cw_carrier_tools(ui);
                    },
                );

                popup_section(
                    ui,
                    "Sidetone envelope",
                    Some("Softens key-up/key-down edges after the BFO"),
                    |ui| {
                        stage_toggle(
                            ui,
                            &mut self.radio.cw.sidetone_envelope.enabled,
                            "Envelope",
                            Some("Key-edge shaping on demod audio"),
                            None,
                            None,
                        );
                        if self.radio.cw.sidetone_envelope.enabled {
                            scroll_slider_f32(
                                ui,
                                &mut self.radio.cw.sidetone_envelope.rise_ms,
                                0.5..=12.0,
                                "Rise ms",
                            );
                            scroll_slider_f32(
                                ui,
                                &mut self.radio.cw.sidetone_envelope.fall_ms,
                                0.5..=20.0,
                                "Fall ms",
                            );
                            let shape_sel = match self.radio.cw.sidetone_envelope.shape {
                                SidetoneEnvelopeShape::Cosine => 0,
                                SidetoneEnvelopeShape::Linear => 1,
                                SidetoneEnvelopeShape::Exponential => 2,
                            };
                            if let Some(i) = labeled_segment_choice(
                                ui,
                                "st_envelope_shape",
                                "Edge shape",
                                shape_sel,
                                &["Cosine", "Linear", "Exponential"],
                                36.0,
                            ) {
                                self.radio.cw.sidetone_envelope.shape = match i {
                                    1 => SidetoneEnvelopeShape::Linear,
                                    2 => SidetoneEnvelopeShape::Exponential,
                                    _ => SidetoneEnvelopeShape::Cosine,
                                };
                            }
                            let shape_hint = match self.radio.cw.sidetone_envelope.shape {
                                SidetoneEnvelopeShape::Cosine => {
                                    "Smooth cosine ramps — least clicky (default)"
                                }
                                SidetoneEnvelopeShape::Linear => {
                                    "Constant slope — sharper attack and release"
                                }
                                SidetoneEnvelopeShape::Exponential => {
                                    "Fast attack ease-out — sharpest edges, most clicky"
                                }
                            };
                            ui.label(egui::RichText::new(shape_hint).small().color(MUTED));
                        } else {
                            ui.label(
                                egui::RichText::new(
                                    "Off — raw BFO product edges (may click on fast keying)",
                                )
                                .small()
                                .color(MUTED),
                            );
                        }
                    },
                );

                popup_section(
                    ui,
                    "Channel filter",
                    Some("IQ bandpass before demod — BW is width; Filter design is skirt shape"),
                    |ui| {
                        if !simple {
                            let wide_sel = usize::from(self.radio.passband_wide);
                            if let Some(i) = labeled_segment_choice(
                                ui,
                                "filter_passband",
                                "Passband range",
                                wide_sel,
                                &["CW", "Wide"],
                                36.0,
                            ) {
                                let was_wide = self.radio.passband_wide;
                                self.radio.passband_wide = i == 1;
                                if !self.radio.passband_wide {
                                    self.radio.cw.full_demod = true;
                                }
                                if self.radio.passband_wide && !was_wide
                                    && self.radio.cw.passband_hz < CW_PASSBAND_NARROW_MAX_HZ
                                {
                                    self.radio.cw.passband_hz = CW_PASSBAND_NARROW_MAX_HZ;
                                }
                            }
                        }
                        let bw_max = self.passband_max_hz();
                        let bw_min = if self.radio.passband_wide {
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
                        if !simple {
                            let delay_note =
                                if self.radio.cw.effective_channel_filter() == ChannelFilterKind::LinearFir
                                {
                                    let delay_ms =
                                        channel_group_delay_ms(audio_rate, self.radio.cw.passband_hz);
                                    let shape_hint = match self.radio.cw.window {
                                        WindowKind::Gaussian => {
                                            " · Blackman/Kaiser reject skirt noise better"
                                        }
                                        WindowKind::RaisedCosine => {
                                            " · Blackman is steeper on adjacent carriers"
                                        }
                                        _ => "",
                                    };
                                    format!(
                                        "Filter delay ~{delay_ms:.0} ms (linear-phase FIR){shape_hint}"
                                    )
                                } else {
                                    let kind = match self.radio.cw.iir_filter {
                                        IirFilterKind::Chebyshev => "Chebyshev",
                                        IirFilterKind::Butterworth => "Butterworth",
                                    };
                                    format!(
                                        "{kind} IIR 2-pole — minimal delay, non-linear phase (may ring)"
                                    )
                                };
                            ui.label(egui::RichText::new(delay_note).small().color(MUTED));

                            let filter_advanced =
                                self.radio.cw.channel_filter != ChannelFilterKind::LinearFir
                                    || self.radio.cw.iir_filter != IirFilterKind::Butterworth
                                    || self.radio.cw.window != WindowKind::RaisedCosine
                                    || self.radio.cw.passband_flatten
                                    || self.radio.cw.window == WindowKind::Kaiser;
                            let design_hdr = egui::CollapsingHeader::new(
                                egui::RichText::new("Filter design").small().color(MUTED),
                            )
                            .id_salt("cw_filter_design")
                            .default_open(filter_advanced)
                            .show(ui, |ui| {
                                let audio_rate = hfsdr::audio_sample_rate(
                                    self.radio.sample_rate,
                                    self.radio.cw.decimation,
                                );
                                crate::filter_design_panel::show_filter_design_panel(
                                    ui,
                                    crate::filter_design_panel::FilterDesignPanel {
                                        settings: &mut self.radio.cw,
                                        audio_rate,
                                    },
                                );
                            });
                            attach_rich_tooltip(
                                &design_hdr.header_response,
                                Some("Filter design"),
                                &[
                                    ("How the bandpass is built", ACCENT),
                                    (
                                        "BW sets passband width. Architecture and window set skirt \
                                         steepness — the response plot above updates live as you change settings.",
                                        MUTED,
                                    ),
                                ],
                            );
                        }
                    },
                );

                popup_section(ui, "Level (AGC)", Some("IQ envelope gain before demod"), |ui| {
                    self.agc_controls(ui, !simple);
                });
            },
        );
    }

    pub(crate) fn agc_controls(&mut self, ui: &mut egui::Ui, advanced: bool) {
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
                AgcMode::Lookahead => 3,
            };
            if let Some(i) = labeled_segment_choice(
                ui,
                "agc_mode",
                "Mode",
                mode_sel,
                &["Envelope", "Hang", "Dual-loop", "Lookahead"],
                36.0,
            ) {
                self.radio.cw.agc_mode = match i {
                    1 => AgcMode::Hang,
                    2 => AgcMode::DualLoop,
                    3 => AgcMode::Lookahead,
                    _ => AgcMode::Envelope,
                };
            }
            let mode_hint = match self.radio.cw.agc_mode {
                AgcMode::Envelope => "Symmetric attack/decay — general-purpose level riding",
                AgcMode::Hang => "Fast gain reduction, slow recovery — quieter between dits",
                AgcMode::DualLoop => "Peak + floor trackers — resists neighbour-signal pumping",
                AgcMode::Lookahead => {
                    "Forward peak scan + slow gain ramps — pre-ducks before peaks, fewer clicks"
                }
            };
            ui.label(egui::RichText::new(mode_hint).small().color(MUTED));
            if advanced {
                scroll_slider_f32(ui, &mut self.radio.cw.agc.attack_ms, 1.0..=20.0, "Attack ms");
                scroll_slider_f32(ui, &mut self.radio.cw.agc.decay_ms, 20.0..=600.0, "Decay ms");
                scroll_slider_f32(ui, &mut self.radio.cw.agc.target, 0.05..=0.6, "Target");
                if self.radio.cw.agc_mode == AgcMode::Lookahead {
                    scroll_slider_f32(
                        ui,
                        &mut self.radio.cw.agc.lookahead_ms,
                        1.0..=25.0,
                        "Lookahead ms",
                    );
                }
            }
        } else {
            scroll_slider_f32(ui, &mut self.radio.cw.agc.manual_gain, 0.1..=16.0, "Manual gain");
        }
    }
}
