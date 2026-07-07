use crate::app::WaterfallApp;
use crate::app::prelude::*;

impl WaterfallApp {

    pub(crate) fn af_tuning_card(&mut self, ui: &mut egui::Ui) {
        if !self.chrome.show_af_scope {
            return;
        }
        section_card(ui, |ui| {
            section_heading_with_tip(
                ui,
                "AF tuning",
                &[
                    ("Goal", ACCENT),
                    ("Tune RF gain so the AF envelope sits near ±half scale.", MUTED),
                    ("Status badge", OK),
                    (
                        "LOW / OK / HOT reflects RF level, IQ AGC headroom, and AF peak.",
                        MUTED,
                    ),
                ],
            );
            let streaming = matches!(self.engine_ui.conn_state, ConnState::Streaming);
            let hint = classify_level(
                self.engine_ui.stats.audio_peak,
                self.radio.cw.agc.enabled,
                self.engine_ui.stats.agc_gain,
                self.engine_ui.stats.agc_envelope,
                self.radio.cw.agc.target,
                streaming,
            );
            meters::show_af_tuning_panel(
                ui,
                &mut self.meter_display.af_scope_view,
                &AfScopeParams {
                    envelope: self.meter_display.af_scope.envelope(),
                    waveform: &self.audio.audio_waveform,
                    peak: self.engine_ui.stats.audio_peak,
                    peak_display: self.meter_display.display.af_scope_peak,
                    rms: self.engine_ui.stats.audio_rms,
                    agc_gain: self.engine_ui.stats.agc_gain,
                    agc_envelope: self.engine_ui.stats.agc_envelope,
                    agc_enabled: self.radio.cw.agc.enabled,
                    agc_target: self.radio.cw.agc.target,
                    iq_headroom: self.engine_ui.stats.iq_buffer_fill,
                    rssi_dbm: self.engine_ui.stats.rssi_dbm,
                    iq_rf_level: self.engine_ui.stats.iq_rf_level,
                    streaming,
                    hint,
                },
            );
        });
    }



}
