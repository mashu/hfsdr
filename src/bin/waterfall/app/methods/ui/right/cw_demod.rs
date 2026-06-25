use crate::app::WaterfallApp;
use crate::app::prelude::*;

impl WaterfallApp {

    pub(crate) fn cw_carrier_tools(&mut self, ui: &mut egui::Ui) {
        let bfo = self.radio.cw.bfo_hz.round();
        ui.horizontal(|ui| {
            if ui
                .button(format!("Zero-beat (Z) → {bfo:.0} Hz"))
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


    pub(crate) fn cw_demod_card(&mut self, ui: &mut egui::Ui) {
        section_card(ui, |ui| {
            section_heading_with_tip(
                ui,
                "CW demod",
                &[
                    ("Channel filter", ACCENT),
                    (
                        "Complex IQ filter before demod — rejects adjacent signals while the carrier is still recoverable.",
                        MUTED,
                    ),
                    ("Plot", ACCENT),
                    (
                        "Ctrl+scroll: BW · drag cyan band = RIT · cyan edges = width · purple notches draggable.",
                        MUTED,
                    ),
                ],
            );
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new("BFO").small().color(MUTED));
                for (label, hz) in BFO_PRESETS {
                    if ui.selectable_label(self.radio.cw.bfo_hz.round() == hz, label).clicked() {
                        self.radio.cw.bfo_hz = hz;
                    }
                }
            });
            scroll_slider_f32_step(ui, &mut self.radio.cw.bfo_hz, 300.0..=1_200.0, "BFO tone", 10.0);
            self.cw_carrier_tools(ui);
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.skimmer_ui.filter_wide, false, "CW (≤500 Hz)");
                ui.selectable_value(&mut self.skimmer_ui.filter_wide, true, "Wide (≤2 kHz)");
            });
            let bw_max = self.passband_max_hz();
            if self.radio.cw.passband_hz > bw_max {
                self.radio.cw.passband_hz = bw_max;
            }
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new("BW").small().color(MUTED));
                for (label, hz) in FILTER_PRESETS {
                    if hz > bw_max {
                        continue;
                    }
                    if ui.selectable_label(self.radio.cw.passband_hz.round() == hz, label).clicked() {
                        self.radio.cw.passband_hz = hz;
                    }
                }
            });
            scroll_slider_log_f32(
                ui,
                &mut self.radio.cw.passband_hz,
                CW_PASSBAND_MIN_HZ..=bw_max,
                "Channel filter",
            );
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Architecture").small().color(MUTED));
                if ui
                    .selectable_label(
                        self.radio.cw.channel_filter == ChannelFilterKind::LinearFir,
                        "FIR (linear)",
                    )
                    .on_hover_text("Linear-phase windowed sinc — best CW keying, tunable shape")
                    .clicked()
                {
                    self.radio.cw.channel_filter = ChannelFilterKind::LinearFir;
                }
                if ui
                    .selectable_label(
                        self.radio.cw.channel_filter == ChannelFilterKind::Iir2Pole,
                        "IIR 2-pole",
                    )
                    .on_hover_text("Biquad lowpass — steeper skirts, may ring on edges (A/B)")
                    .clicked()
                {
                    self.radio.cw.channel_filter = ChannelFilterKind::Iir2Pole;
                }
            });
            if self.radio.cw.channel_filter == ChannelFilterKind::LinearFir {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Shape").small().color(MUTED));
                    window_choice(
                        ui,
                        &mut self.radio.cw.window,
                        WindowKind::Gaussian,
                        "Gauss",
                        "Softest tone, gentle skirts — clean signals, minimal ringing",
                    );
                    window_choice(
                        ui,
                        &mut self.radio.cw.window,
                        WindowKind::RaisedCosine,
                        "RaisedCos",
                        "Balanced default — good tone with moderate adjacent rejection",
                    );
                    window_choice(
                        ui,
                        &mut self.radio.cw.window,
                        WindowKind::Blackman,
                        "Blackman",
                        "Steepest skirts — reject nearby QRM before narrowing bandwidth",
                    );
                    window_choice(
                        ui,
                        &mut self.radio.cw.window,
                        WindowKind::Kaiser,
                        "Kaiser",
                        "Tunable β — flat passband vs steep skirts (adjust β below)",
                    );
                });
                if self.radio.cw.window == WindowKind::Kaiser {
                    scroll_slider_f32(ui, &mut self.radio.cw.kaiser_beta, 2.0..=14.0, "Kaiser β");
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
            let audio_rate = hfsdr::audio_sample_rate(self.radio.sample_rate, self.radio.cw.decimation);
            let delay_note = if self.radio.cw.channel_filter == ChannelFilterKind::LinearFir {
                let delay_ms = channel_group_delay_ms(audio_rate, self.radio.cw.passband_hz);
                format!("Filter delay ~{delay_ms:.0} ms (linear-phase FIR)")
            } else {
                "IIR 2-pole — minimal delay, non-linear phase (may ring)".to_string()
            };
            ui.label(egui::RichText::new(delay_note).small().color(MUTED));
            self.agc_controls(ui);
        });
    }


    pub(crate) fn agc_controls(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.label(egui::RichText::new("④ Level — IQ envelope, before demod").small().color(MUTED));
        stage_toggle(
            ui,
            &mut self.radio.cw.agc.enabled,
            "AGC",
            Some("IQ envelope gain riding"),
            Some("A"),
            None,
        );
        if self.radio.cw.agc.enabled {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Mode").small().color(MUTED));
                if ui
                    .selectable_label(self.radio.cw.agc_mode == AgcMode::Envelope, "Envelope")
                    .on_hover_text("Symmetric attack/decay — general-purpose; gain follows IQ level evenly")
                    .clicked()
                {
                    self.radio.cw.agc_mode = AgcMode::Envelope;
                }
                if ui
                    .selectable_label(self.radio.cw.agc_mode == AgcMode::Hang, "Hang")
                    .on_hover_text(
                        "Fast gain reduction, slow recovery — less noise lift between dits; \
                         most audible vs Envelope on weak CW with band noise",
                    )
                    .clicked()
                {
                    self.radio.cw.agc_mode = AgcMode::Hang;
                }
                if ui
                    .selectable_label(self.radio.cw.agc_mode == AgcMode::DualLoop, "Dual-loop")
                    .on_hover_text(
                        "Fast peak + slow floor trackers — resists pumping from strong neighbours; \
                         try when Envelope breathes on QRM",
                    )
                    .clicked()
                {
                    self.radio.cw.agc_mode = AgcMode::DualLoop;
                }
            });
            scroll_slider_f32(ui, &mut self.radio.cw.agc.attack_ms, 1.0..=20.0, "Attack ms");
            scroll_slider_f32(ui, &mut self.radio.cw.agc.decay_ms, 20.0..=600.0, "Decay ms");
            scroll_slider_f32(ui, &mut self.radio.cw.agc.target, 0.05..=0.6, "Target");
        } else {
            scroll_slider_f32(ui, &mut self.radio.cw.agc.manual_gain, 0.1..=16.0, "Manual gain");
        }
    }



}
