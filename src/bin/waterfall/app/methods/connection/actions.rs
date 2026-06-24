// `connection/actions` — connect, disconnect, and recent-host bookkeeping.

    fn apply_connect_form(&mut self, req: &ConnectRequest) {
        self.form_kind = req.kind;
        self.form_host = req.host.clone();
        self.form_port = req.port;
        self.form_kiwi = req.kiwi.clone();
        if req.kind == SourceKind::Kiwi {
            self.agc_rf_on = req.kiwi.rf_agc_on;
            self.last_agc_rf_on = req.kiwi.rf_agc_on;
        }
        if req.sample_rate != 0 {
            self.form_sample_rate = req.sample_rate;
        }
        self.form_airspy = req.airspy.clone();
        self.form_rtlsdr = req.rtlsdr.clone();
        self.form_qmx = req.qmx.clone();
    }

    fn can_connect_request(req: &ConnectRequest) -> bool {
        is_local_source(req.kind) || !req.host.trim().is_empty()
    }

    fn can_quick_connect(&self) -> bool {
        if let Some(req) = self.recent_hosts.first() {
            Self::can_connect_request(req)
        } else {
            is_local_source(self.form_kind) || !self.form_host.trim().is_empty()
        }
    }

    fn quick_connect_target_label(&self) -> String {
        self.recent_hosts
            .first()
            .map(|r| r.label())
            .unwrap_or_else(|| self.connection_alias())
    }

    fn quick_connect_last(&mut self) {
        if let Some(req) = self.recent_hosts.first().cloned() {
            self.apply_connect_form(&req);
        }
        self.connect_now();
    }

    fn connect_now(&mut self) {
        self.clamp_center_to_ham_bands();
        let sample_rate = match self.form_kind {
            #[cfg(feature = "airspy")]
            SourceKind::Airspy => self.form_sample_rate,
            #[cfg(feature = "rtlsdr")]
            SourceKind::RtlSdr => self.form_sample_rate,
            #[cfg(feature = "qmx")]
            SourceKind::Qmx => 0,
            _ => 0,
        };
        let mut kiwi = self.form_kiwi.clone();
        kiwi.rf_agc_on = self.agc_rf_on;
        let req = ConnectRequest {
            kind: self.form_kind,
            host: self.form_host.trim().to_string(),
            port: self.form_port,
            center_hz: self.center_khz * 1000.0,
            sample_rate,
            kiwi,
            airspy: self.form_airspy.clone(),
            rtlsdr: self.form_rtlsdr.clone(),
            qmx: self.form_qmx.clone(),
        };
        self.last_airspy_rf = self.form_airspy.clone();
        self.last_rtlsdr_rf = self.form_rtlsdr.clone();
        self.last_qmx_rf = self.form_qmx.clone();
        self.last_kiwi_man_gain = self.form_kiwi.man_gain;
        self.last_kiwi_rf_attn_db = self.form_kiwi.rf_attn_db;
        self.last_kiwi_has_rf_attn = false;
        self.last_agc_rf_on = !self.agc_rf_on;
        self.last_center_khz = self.center_khz;
        self.remember_host(&req);
        self.apply_default_view_zoom();
        log::info(format!("connecting to {}", req.label()));
        self.engine.send(EngineCommand::Connect(req));
    }

    fn cancel_connection(&mut self) {
        self.engine.abort_connect();
        self.engine.send(EngineCommand::Disconnect);
    }

    fn remember_host(&mut self, req: &ConnectRequest) {
        self.recent_hosts.retain(|r| r != req);
        self.recent_hosts.insert(0, req.clone());
        self.recent_hosts.truncate(8);
    }
