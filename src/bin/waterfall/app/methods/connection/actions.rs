use crate::app::WaterfallApp;
use crate::app::prelude::*;

impl WaterfallApp {

    pub(crate) fn apply_connect_form(&mut self, req: &ConnectRequest) {
        self.connection.form.kind = req.kind;
        self.connection.form.host = req.host.clone();
        self.connection.form.port = req.port;
        self.connection.form.kiwi = req.kiwi.clone();
        if req.kind == SourceKind::Kiwi {
            self.radio.agc_rf_on = req.kiwi.rf_agc_on;
            self.radio.last_agc_rf_on = req.kiwi.rf_agc_on;
        }
        if req.sample_rate != 0 {
            self.connection.form.sample_rate = req.sample_rate;
        }
        self.connection.form.airspy = req.airspy.clone();
        self.connection.form.rtlsdr = req.rtlsdr.clone();
        self.connection.form.qmx = req.qmx.clone();
    }

    pub(crate) fn can_connect_request(req: &ConnectRequest) -> bool {
        is_local_source(req.kind) || !req.host.trim().is_empty()
    }

    pub(crate) fn can_quick_connect(&self) -> bool {
        if let Some(req) = self.connection.form.recent_hosts.first() {
            Self::can_connect_request(req)
        } else {
            is_local_source(self.connection.form.kind) || !self.connection.form.host.trim().is_empty()
        }
    }

    pub(crate) fn quick_connect_target_label(&self) -> String {
        self.connection.form.recent_hosts
            .first()
            .map(|r| r.label())
            .unwrap_or_else(|| self.connection_alias())
    }

    pub(crate) fn quick_connect_last(&mut self) {
        if let Some(req) = self.connection.form.recent_hosts.first().cloned() {
            self.apply_connect_form(&req);
        }
        self.connect_now();
    }

    pub(crate) fn connect_now(&mut self) {
        self.clamp_center_to_ham_bands();
        let sample_rate = match self.connection.form.kind {
            #[cfg(feature = "airspy")]
            SourceKind::Airspy => self.connection.form.sample_rate,
            #[cfg(feature = "rtlsdr")]
            SourceKind::RtlSdr => self.connection.form.sample_rate,
            #[cfg(feature = "qmx")]
            SourceKind::Qmx => 0,
            _ => 0,
        };
        let mut kiwi = self.connection.form.kiwi.clone();
        kiwi.rf_agc_on = self.radio.agc_rf_on;
        let req = ConnectRequest {
            kind: self.connection.form.kind,
            host: self.connection.form.host.trim().to_string(),
            port: self.connection.form.port,
            center_hz: self.radio.center_khz * 1000.0,
            sample_rate,
            kiwi,
            airspy: self.connection.form.airspy.clone(),
            rtlsdr: self.connection.form.rtlsdr.clone(),
            qmx: self.connection.form.qmx.clone(),
        };
        self.connection.form.last_airspy_rf = self.connection.form.airspy.clone();
        self.connection.form.last_rtlsdr_rf = self.connection.form.rtlsdr.clone();
        self.connection.form.last_qmx_rf = self.connection.form.qmx.clone();
        self.radio.last_kiwi_man_gain = self.connection.form.kiwi.man_gain;
        self.radio.last_kiwi_rf_attn_db = self.connection.form.kiwi.rf_attn_db;
        self.radio.last_kiwi_has_rf_attn = false;
        self.radio.last_agc_rf_on = !self.radio.agc_rf_on;
        self.radio.last_center_khz = self.radio.center_khz;
        self.remember_host(&req);
        self.apply_default_view_zoom();
        log::info(format!("connecting to {}", req.label()));
        self.engine.send(EngineCommand::Connect(req));
    }

    pub(crate) fn cancel_connection(&mut self) {
        self.engine.abort_connect();
        self.engine.send(EngineCommand::Disconnect);
    }

    pub(crate) fn remember_host(&mut self, req: &ConnectRequest) {
        self.connection.form.recent_hosts.retain(|r| r != req);
        self.connection.form.recent_hosts.insert(0, req.clone());
        self.connection.form.recent_hosts.truncate(8);
    }

}
