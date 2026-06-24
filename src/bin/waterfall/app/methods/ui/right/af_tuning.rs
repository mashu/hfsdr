// `ui/right/af_tuning` — AF scope card.

    fn af_tuning_card(&mut self, ui: &mut egui::Ui) {
        if !self.show_af_scope {
            return;
        }
        section_card(ui, |ui| {
            section_heading_with_tip(
                ui,
                "AF tuning",
                &[
                    ("Goal", ACCENT),
                    ("Tune RF gain so the AF trace sits near ±half scale.", MUTED),
                    ("Status badge", OK),
                    (
                        "LOW / OK / HOT reflects RF level, IQ AGC headroom, and AF peak.",
                        MUTED,
                    ),
                ],
            );
            let streaming = matches!(self.conn_state, ConnState::Streaming);
            let hint = classify_level(
                self.stats.audio_peak,
                self.cw.agc.enabled,
                self.stats.agc_gain,
                self.stats.agc_envelope,
                self.cw.agc.target,
                streaming,
            );
            meters::show_af_tuning_panel(
                ui,
                &AfScopeParams {
                    samples: &self.audio_scope,
                    peak: self.stats.audio_peak,
                    rms: self.stats.audio_rms,
                    agc_gain: self.stats.agc_gain,
                    agc_envelope: self.stats.agc_envelope,
                    agc_enabled: self.cw.agc.enabled,
                    agc_target: self.cw.agc.target,
                    iq_headroom: self.stats.iq_buffer_fill,
                    rssi_dbm: self.stats.rssi_dbm,
                    iq_rf_level: self.stats.iq_rf_level,
                    streaming,
                    hint,
                },
            );
        });
    }


