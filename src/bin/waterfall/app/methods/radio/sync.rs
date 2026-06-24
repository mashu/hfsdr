// `radio/sync` — push live tuning and RF parameters to the engine.

    fn apply_audio_device(&mut self) {
        if self.selected_audio_device == self.last_audio_device {
            return;
        }
        let name = self.audio_devices.get(self.selected_audio_device).cloned();
        self.engine.send(EngineCommand::SetAudioDevice(name));
        self.last_audio_device = self.selected_audio_device;
    }

    fn kiwi_rf_live(&self) -> bool {
        self.form_kind == SourceKind::Kiwi && matches!(self.conn_state, ConnState::Streaming)
    }

    fn sync_kiwi_rf_now(&mut self) {
        if !self.kiwi_rf_live() {
            return;
        }
        let mut rf_changed = false;
        if self.agc_rf_on != self.last_agc_rf_on {
            self.engine.send(EngineCommand::SetRfAgc(self.agc_rf_on));
            self.last_agc_rf_on = self.agc_rf_on;
            self.form_kiwi.rf_agc_on = self.agc_rf_on;
            rf_changed = true;
        }
        if self.form_kiwi.man_gain != self.last_kiwi_man_gain {
            self.engine
                .send(EngineCommand::SetKiwiManGain(self.form_kiwi.man_gain));
            self.last_kiwi_man_gain = self.form_kiwi.man_gain;
            rf_changed = true;
        }
        if rf_changed {
            self.lock_display_levels_for_rf_tuning();
        }
    }

    fn rf_meter_dbm(&self) -> f32 {
        rf_level_dbm(self.stats.rssi_dbm, self.stats.iq_rf_level)
    }

    fn apply_radio_settings(&mut self) {
        if (self.center_khz - self.last_center_khz).abs() > f64::EPSILON {
            self.invalidate_waterfall_history();
            self.engine.send(EngineCommand::Tune(self.center_khz * 1000.0));
            self.last_center_khz = self.center_khz;
        }
        self.sync_kiwi_rf_now();
        self.apply_kiwi_rf_attn_settings();
        self.apply_airspy_live_settings();
        self.apply_rtlsdr_live_settings();
        self.apply_qmx_live_settings();
        self.apply_audio_device();
    }

    fn apply_kiwi_rf_attn_settings(&mut self) {
        if !self.kiwi_rf_live() {
            return;
        }
        if self.stats.kiwi_has_rf_attn && !self.last_kiwi_has_rf_attn {
            self.engine
                .send(EngineCommand::SetKiwiRfAttn(self.form_kiwi.rf_attn_db));
            self.last_kiwi_rf_attn_db = self.form_kiwi.rf_attn_db;
        }
        self.last_kiwi_has_rf_attn = self.stats.kiwi_has_rf_attn;
        if !self.stats.kiwi_has_rf_attn {
            return;
        }
        let db = self.form_kiwi.rf_attn_db;
        if (db - self.last_kiwi_rf_attn_db).abs() > 0.05 {
            self.engine.send(EngineCommand::SetKiwiRfAttn(db));
            self.last_kiwi_rf_attn_db = db;
            self.lock_display_levels_for_rf_tuning();
        }
    }

    fn apply_qmx_live_settings(&mut self) {
        #[cfg(not(feature = "qmx"))]
        {
            let _ = self;
            return;
        }
        #[cfg(feature = "qmx")]
        {
            if self.is_kiwi || !matches!(self.conn_state, ConnState::Streaming) {
                return;
            }
            if self.form_kind != SourceKind::Qmx {
                return;
            }
            if self.form_qmx.rf_gain_db != self.last_qmx_rf.rf_gain_db {
                self.engine
                    .send(EngineCommand::SetQmxRfGain(self.form_qmx.rf_gain_db));
                self.last_qmx_rf.rf_gain_db = self.form_qmx.rf_gain_db;
                self.lock_display_levels_for_rf_tuning();
            }
        }
    }

    fn apply_rtlsdr_live_settings(&mut self) {
        #[cfg(not(feature = "rtlsdr"))]
        {
            let _ = self;
            return;
        }
        #[cfg(feature = "rtlsdr")]
        {
            if self.is_kiwi || !matches!(self.conn_state, ConnState::Streaming) {
                return;
            }
            if self.form_kind != SourceKind::RtlSdr {
                return;
            }
            if self.form_rtlsdr.rtl_agc != self.last_rtlsdr_rf.rtl_agc {
                self.engine
                    .send(EngineCommand::SetRtlSdrRtlAgc(self.form_rtlsdr.rtl_agc));
                self.last_rtlsdr_rf.rtl_agc = self.form_rtlsdr.rtl_agc;
                self.lock_display_levels_for_rf_tuning();
            }
            if self.form_rtlsdr.manual_gain != self.last_rtlsdr_rf.manual_gain {
                self.engine
                    .send(EngineCommand::SetRtlSdrManualGain(self.form_rtlsdr.manual_gain));
                self.last_rtlsdr_rf.manual_gain = self.form_rtlsdr.manual_gain;
                self.lock_display_levels_for_rf_tuning();
            }
            if self.form_rtlsdr.manual_gain
                && self.form_rtlsdr.tuner_gain_db10 != self.last_rtlsdr_rf.tuner_gain_db10
            {
                self.engine.send(EngineCommand::SetRtlSdrTunerGain(
                    self.form_rtlsdr.tuner_gain_db10,
                ));
                self.last_rtlsdr_rf.tuner_gain_db10 = self.form_rtlsdr.tuner_gain_db10;
                self.lock_display_levels_for_rf_tuning();
            }
            if self.form_rtlsdr.bias_tee != self.last_rtlsdr_rf.bias_tee {
                self.engine
                    .send(EngineCommand::SetRtlSdrBiasTee(self.form_rtlsdr.bias_tee));
                self.last_rtlsdr_rf.bias_tee = self.form_rtlsdr.bias_tee;
            }
            if self.form_rtlsdr.ppm != self.last_rtlsdr_rf.ppm {
                self.engine
                    .send(EngineCommand::SetRtlSdrPpm(self.form_rtlsdr.ppm));
                self.last_rtlsdr_rf.ppm = self.form_rtlsdr.ppm;
            }
        }
    }

    fn apply_airspy_live_settings(&mut self) {
        #[cfg(not(feature = "airspy"))]
        {
            let _ = self;
            return;
        }
        #[cfg(feature = "airspy")]
        {
            if self.is_kiwi || !matches!(self.conn_state, ConnState::Streaming) {
                return;
            }
            if self.form_kind != SourceKind::Airspy {
                return;
            }
            if self.form_airspy.hf_agc != self.last_airspy_rf.hf_agc {
                self.engine
                    .send(EngineCommand::SetRfAgc(self.form_airspy.hf_agc));
                self.last_airspy_rf.hf_agc = self.form_airspy.hf_agc;
                self.lock_display_levels_for_rf_tuning();
            }
            if self.form_airspy.hf_agc_threshold_high != self.last_airspy_rf.hf_agc_threshold_high {
                self.engine.send(EngineCommand::SetAirspyAgcThreshold(
                    self.form_airspy.hf_agc_threshold_high,
                ));
                self.last_airspy_rf.hf_agc_threshold_high = self.form_airspy.hf_agc_threshold_high;
            }
            if self.form_airspy.hf_att != self.last_airspy_rf.hf_att {
                self.engine
                    .send(EngineCommand::SetAirspyAtt(self.form_airspy.hf_att));
                self.last_airspy_rf.hf_att = self.form_airspy.hf_att;
                self.lock_display_levels_for_rf_tuning();
            }
            if self.form_airspy.hf_lna != self.last_airspy_rf.hf_lna {
                self.engine
                    .send(EngineCommand::SetAirspyLna(self.form_airspy.hf_lna));
                self.last_airspy_rf.hf_lna = self.form_airspy.hf_lna;
                self.lock_display_levels_for_rf_tuning();
            }
            let frontend = self.form_airspy.frontend_flags();
            if frontend != self.last_airspy_rf.frontend_flags() {
                self.engine
                    .send(EngineCommand::SetAirspyFrontendOptions(frontend));
                self.last_airspy_rf.frontend_optimize_band_iii =
                    self.form_airspy.frontend_optimize_band_iii;
                self.last_airspy_rf.frontend_optimize_pll_boundary =
                    self.form_airspy.frontend_optimize_pll_boundary;
            }
            if self.form_airspy.bias_tee != self.last_airspy_rf.bias_tee {
                self.engine
                    .send(EngineCommand::SetAirspyBiasTee(self.form_airspy.bias_tee));
                self.last_airspy_rf.bias_tee = self.form_airspy.bias_tee;
            }
        }
    }
