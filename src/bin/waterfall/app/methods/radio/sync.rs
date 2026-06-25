use crate::app::WaterfallApp;
use crate::app::prelude::*;

impl WaterfallApp {

    pub(crate) fn apply_audio_device(&mut self) {
        if self.audio.selected_audio_device == self.audio.last_audio_device {
            return;
        }
        let name = self.audio.audio_devices.get(self.audio.selected_audio_device).cloned();
        self.engine.send(EngineCommand::SetAudioDevice(name));
        self.audio.last_audio_device = self.audio.selected_audio_device;
    }

    pub(crate) fn kiwi_rf_live(&self) -> bool {
        self.connection.form.kind == SourceKind::Kiwi && matches!(self.engine_ui.conn_state, ConnState::Streaming)
    }

    pub(crate) fn sync_kiwi_rf_now(&mut self) {
        if !self.kiwi_rf_live() {
            return;
        }
        let mut rf_changed = false;
        if self.radio.agc_rf_on != self.radio.last_agc_rf_on {
            self.engine.send(EngineCommand::SetRfAgc(self.radio.agc_rf_on));
            self.radio.last_agc_rf_on = self.radio.agc_rf_on;
            self.connection.form.kiwi.rf_agc_on = self.radio.agc_rf_on;
            rf_changed = true;
        }
        if self.connection.form.kiwi.man_gain != self.radio.last_kiwi_man_gain {
            self.engine
                .send(EngineCommand::SetKiwiManGain(self.connection.form.kiwi.man_gain));
            self.radio.last_kiwi_man_gain = self.connection.form.kiwi.man_gain;
            rf_changed = true;
        }
        if rf_changed {
            self.lock_display_levels_for_rf_tuning();
        }
    }

    pub(crate) fn rf_meter_dbm(&self) -> f32 {
        rf_level_dbm(self.engine_ui.stats.rssi_dbm, self.engine_ui.stats.iq_rf_level)
    }

    pub(crate) fn apply_radio_settings(&mut self) {
        if (self.radio.center_khz - self.radio.last_center_khz).abs() > f64::EPSILON {
            self.invalidate_waterfall_history();
            self.engine.send(EngineCommand::Tune(self.radio.center_khz * 1000.0));
            self.radio.last_center_khz = self.radio.center_khz;
        }
        self.sync_kiwi_rf_now();
        self.apply_kiwi_rf_attn_settings();
        self.apply_airspy_live_settings();
        self.apply_rtlsdr_live_settings();
        self.apply_qmx_live_settings();
        self.apply_audio_device();
    }

    pub(crate) fn apply_kiwi_rf_attn_settings(&mut self) {
        if !self.kiwi_rf_live() {
            return;
        }
        if self.engine_ui.stats.kiwi_has_rf_attn && !self.radio.last_kiwi_has_rf_attn {
            self.engine
                .send(EngineCommand::SetKiwiRfAttn(self.connection.form.kiwi.rf_attn_db));
            self.radio.last_kiwi_rf_attn_db = self.connection.form.kiwi.rf_attn_db;
        }
        self.radio.last_kiwi_has_rf_attn = self.engine_ui.stats.kiwi_has_rf_attn;
        if !self.engine_ui.stats.kiwi_has_rf_attn {
            return;
        }
        let db = self.connection.form.kiwi.rf_attn_db;
        if (db - self.radio.last_kiwi_rf_attn_db).abs() > 0.05 {
            self.engine.send(EngineCommand::SetKiwiRfAttn(db));
            self.radio.last_kiwi_rf_attn_db = db;
            self.lock_display_levels_for_rf_tuning();
        }
    }

    pub(crate) fn apply_qmx_live_settings(&mut self) {
        #[cfg(not(feature = "qmx"))]
        {
            let _ = self;
            return;
        }
        #[cfg(feature = "qmx")]
        {
            if self.radio.is_kiwi || !matches!(self.engine_ui.conn_state, ConnState::Streaming) {
                return;
            }
            if self.connection.form.kind != SourceKind::Qmx {
                return;
            }
            if self.connection.form.qmx.rf_gain_db != self.connection.form.last_qmx_rf.rf_gain_db {
                self.engine
                    .send(EngineCommand::SetQmxRfGain(self.connection.form.qmx.rf_gain_db));
                self.connection.form.last_qmx_rf.rf_gain_db = self.connection.form.qmx.rf_gain_db;
                self.lock_display_levels_for_rf_tuning();
            }
        }
    }

    pub(crate) fn apply_rtlsdr_live_settings(&mut self) {
        #[cfg(not(feature = "rtlsdr"))]
        {
            let _ = self;
            return;
        }
        #[cfg(feature = "rtlsdr")]
        {
            if self.radio.is_kiwi || !matches!(self.engine_ui.conn_state, ConnState::Streaming) {
                return;
            }
            if self.connection.form.kind != SourceKind::RtlSdr {
                return;
            }
            if self.connection.form.rtlsdr.rtl_agc != self.connection.form.last_rtlsdr_rf.rtl_agc {
                self.engine
                    .send(EngineCommand::SetRtlSdrRtlAgc(self.connection.form.rtlsdr.rtl_agc));
                self.connection.form.last_rtlsdr_rf.rtl_agc = self.connection.form.rtlsdr.rtl_agc;
                self.lock_display_levels_for_rf_tuning();
            }
            if self.connection.form.rtlsdr.manual_gain != self.connection.form.last_rtlsdr_rf.manual_gain {
                self.engine
                    .send(EngineCommand::SetRtlSdrManualGain(self.connection.form.rtlsdr.manual_gain));
                self.connection.form.last_rtlsdr_rf.manual_gain = self.connection.form.rtlsdr.manual_gain;
                self.lock_display_levels_for_rf_tuning();
            }
            if self.connection.form.rtlsdr.manual_gain
                && self.connection.form.rtlsdr.tuner_gain_db10 != self.connection.form.last_rtlsdr_rf.tuner_gain_db10
            {
                self.engine.send(EngineCommand::SetRtlSdrTunerGain(
                    self.connection.form.rtlsdr.tuner_gain_db10,
                ));
                self.connection.form.last_rtlsdr_rf.tuner_gain_db10 = self.connection.form.rtlsdr.tuner_gain_db10;
                self.lock_display_levels_for_rf_tuning();
            }
            if self.connection.form.rtlsdr.bias_tee != self.connection.form.last_rtlsdr_rf.bias_tee {
                self.engine
                    .send(EngineCommand::SetRtlSdrBiasTee(self.connection.form.rtlsdr.bias_tee));
                self.connection.form.last_rtlsdr_rf.bias_tee = self.connection.form.rtlsdr.bias_tee;
            }
            if self.connection.form.rtlsdr.ppm != self.connection.form.last_rtlsdr_rf.ppm {
                self.engine
                    .send(EngineCommand::SetRtlSdrPpm(self.connection.form.rtlsdr.ppm));
                self.connection.form.last_rtlsdr_rf.ppm = self.connection.form.rtlsdr.ppm;
            }
        }
    }

    pub(crate) fn apply_airspy_live_settings(&mut self) {
        #[cfg(not(feature = "airspy"))]
        {
            let _ = self;
            return;
        }
        #[cfg(feature = "airspy")]
        {
            if self.radio.is_kiwi || !matches!(self.engine_ui.conn_state, ConnState::Streaming) {
                return;
            }
            if self.connection.form.kind != SourceKind::Airspy {
                return;
            }
            if self.connection.form.airspy.hf_agc != self.connection.form.last_airspy_rf.hf_agc {
                self.engine
                    .send(EngineCommand::SetRfAgc(self.connection.form.airspy.hf_agc));
                self.connection.form.last_airspy_rf.hf_agc = self.connection.form.airspy.hf_agc;
                self.lock_display_levels_for_rf_tuning();
            }
            if self.connection.form.airspy.hf_agc_threshold_high != self.connection.form.last_airspy_rf.hf_agc_threshold_high {
                self.engine.send(EngineCommand::SetAirspyAgcThreshold(
                    self.connection.form.airspy.hf_agc_threshold_high,
                ));
                self.connection.form.last_airspy_rf.hf_agc_threshold_high = self.connection.form.airspy.hf_agc_threshold_high;
            }
            if self.connection.form.airspy.hf_att != self.connection.form.last_airspy_rf.hf_att {
                self.engine
                    .send(EngineCommand::SetAirspyAtt(self.connection.form.airspy.hf_att));
                self.connection.form.last_airspy_rf.hf_att = self.connection.form.airspy.hf_att;
                self.lock_display_levels_for_rf_tuning();
            }
            if self.connection.form.airspy.hf_lna != self.connection.form.last_airspy_rf.hf_lna {
                self.engine
                    .send(EngineCommand::SetAirspyLna(self.connection.form.airspy.hf_lna));
                self.connection.form.last_airspy_rf.hf_lna = self.connection.form.airspy.hf_lna;
                self.lock_display_levels_for_rf_tuning();
            }
            let frontend = self.connection.form.airspy.frontend_flags();
            if frontend != self.connection.form.last_airspy_rf.frontend_flags() {
                self.engine
                    .send(EngineCommand::SetAirspyFrontendOptions(frontend));
                self.connection.form.last_airspy_rf.frontend_optimize_band_iii =
                    self.connection.form.airspy.frontend_optimize_band_iii;
                self.connection.form.last_airspy_rf.frontend_optimize_pll_boundary =
                    self.connection.form.airspy.frontend_optimize_pll_boundary;
            }
            if self.connection.form.airspy.bias_tee != self.connection.form.last_airspy_rf.bias_tee {
                self.engine
                    .send(EngineCommand::SetAirspyBiasTee(self.connection.form.airspy.bias_tee));
                self.connection.form.last_airspy_rf.bias_tee = self.connection.form.airspy.bias_tee;
            }
        }
    }

}
