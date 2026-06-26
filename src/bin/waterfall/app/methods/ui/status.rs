use crate::app::WaterfallApp;
use crate::app::prelude::*;

impl WaterfallApp {

    pub(crate) fn connection_status_pill(&self) -> (String, Color32) {
        match &self.engine_ui.conn_state {
            ConnState::Streaming if self.connection_unstable() => ("UNSTABLE".to_string(), WARN),
            ConnState::Streaming => ("STREAMING".to_string(), OK),
            ConnState::Reconnecting { attempt, retry_in_s } => {
                (format!("RECONNECT #{attempt} ({retry_in_s:.0}s)"), WARN)
            }
            ConnState::Connecting { .. } => ("CONNECTING".to_string(), WARN),
            ConnState::Disconnected => ("OFFLINE".to_string(), MUTED),
        }
    }



    pub(crate) fn connection_session_live(&self) -> bool {
        matches!(
            self.engine_ui.conn_state,
            ConnState::Streaming | ConnState::Connecting { .. } | ConnState::Reconnecting { .. }
        )
    }



    pub(crate) fn connection_alias(&self) -> String {
        match self.connection.form.kind {
            #[cfg(feature = "airspy")]
            SourceKind::Airspy => "Airspy HF+".to_string(),
            #[cfg(feature = "rtlsdr")]
            SourceKind::RtlSdr => format!("RTL-SDR #{}", self.connection.form.rtlsdr.device_index),
            #[cfg(feature = "qmx")]
            SourceKind::Qmx => {
                if self.connection.form.qmx.serial_port.is_empty() {
                    "QMX".to_string()
                } else {
                    format!("QMX ({})", self.connection.form.qmx.serial_port)
                }
            }
            SourceKind::Kiwi => {
                let host = self.connection.form.host.trim();
                if host.is_empty() {
                    "KiwiSDR".to_string()
                } else {
                    format!("{host}:{}", self.connection.form.port)
                }
            }
        }
    }



    pub(crate) fn status_banner(&mut self, ui: &mut egui::Ui) {
        let conn_label = match &self.engine_ui.conn_state {
            ConnState::Streaming if self.connection_unstable() => "UNSTABLE".to_string(),
            ConnState::Streaming => "STREAMING".to_string(),
            ConnState::Reconnecting { attempt, retry_in_s } => {
                format!("RECONNECT #{attempt} ({retry_in_s:.0}s)")
            }
            ConnState::Connecting { .. } => "CONNECTING".to_string(),
            _ => "OFFLINE".to_string(),
        };
        let conn_color = match &self.engine_ui.conn_state {
            ConnState::Streaming if !self.connection_unstable() => OK,
            ConnState::Disconnected => MUTED,
            _ => WARN,
        };
        let streaming = matches!(self.engine_ui.conn_state, ConnState::Streaming);
        let rx_color = if streaming { ACCENT } else { MUTED };

        ui.horizontal(|ui| {
            let badge_resp = clickable_badge(ui, &conn_label, conn_color)
                .on_hover_text("Click to open connection settings");
            if badge_resp.clicked() {
                self.connection.form.show_connection_drawer = !self.connection.form.show_connection_drawer;
            }
            if self.connection_session_live() {
                let alias_resp =
                    crate::status_widgets::connection_alias_chip(ui, &self.connection_alias());
                if alias_resp.clicked() {
                    self.connection.form.show_connection_drawer = !self.connection.form.show_connection_drawer;
                }
                if crate::status_widgets::disconnect_chip(ui).clicked() {
                    self.cancel_connection();
                }
            } else if matches!(self.engine_ui.conn_state, ConnState::Disconnected) {
                let can_connect = self.can_quick_connect();
                let target = self.quick_connect_target_label();
                let quick_resp = crate::status_widgets::quick_connect_chip(ui, can_connect)
                    .on_hover_text(if can_connect {
                        format!("Quick connect to {target}")
                    } else {
                        "Configure a receiver in connection settings".to_string()
                    });
                if can_connect && quick_resp.clicked() {
                    self.quick_connect_last();
                }
            }

            ui.separator();
            ui.label(
                egui::RichText::new(format!("RX {:.6} MHz", self.radio.center_khz / 1000.0))
                    .strong()
                    .monospace()
                    .color(rx_color),
            );
            ui.label(
                egui::RichText::new(format!("listen {:.0} Hz", self.listen_offset_hz()))
                    .small()
                    .color(MUTED),
            );

            ui.separator();
            ui.label(
                egui::RichText::new(format!("SNR {:.0} dB", self.radio.last_snr_db))
                    .small()
                    .color(MUTED),
            );
            let (cursor_label, cursor_active) = if let Some(offset) = self.plot.hover_offset_hz {
                let cursor_hz = self.center_hz() + offset;
                (
                    format!(
                        "Cursor {}",
                        crate::interaction::format_absolute_freq_hz(cursor_hz)
                    ),
                    true,
                )
            } else {
                ("Cursor —".to_string(), false)
            };
            crate::status_widgets::cursor_freq_slot(ui, &cursor_label, cursor_active);
            let engine_resp = crate::status_widgets::engine_pipeline_chip(
                ui,
                self.chrome.show_pipeline_drawer,
                streaming,
            );
            if engine_resp.clicked() {
                self.chrome.show_pipeline_drawer = !self.chrome.show_pipeline_drawer;
            }
            let filters_active = self.radio.cw.notches.iter().any(|n| n.enabled)
                || !self.radio.cw.diagnostic.channel_fir;
            let filter_resp = crate::status_widgets::filter_diagnostic_chip(
                ui,
                self.chrome.show_filter_drawer,
                filters_active,
            );
            if filter_resp.clicked() {
                self.chrome.show_filter_drawer = !self.chrome.show_filter_drawer;
            }
            let gauge_resp = crate::status_widgets::iq_buffer_control(
                ui,
                self.engine_ui.stats.iq_buffer_fill,
                self.engine_ui.stats.iq_buffer_secs,
                self.chrome.show_iq_drawer,
            );
            if gauge_resp.clicked() {
                self.chrome.show_iq_drawer = !self.chrome.show_iq_drawer;
            }
            let rec_secs = self.engine_ui.stats.iq_capture_samples as f32 / self.engine_ui.stats.sample_rate.max(1.0);
            let rec_resp = crate::status_widgets::iq_record_toggle(
                ui,
                self.engine_ui.stats.iq_recording,
                streaming,
                rec_secs,
            );
            if rec_resp.clicked() {
                if let Some(cmd) = self.chrome.iq.toggle_recording(self.engine_ui.stats.iq_recording, streaming) {
                    self.settings_dirty_at = Some(Instant::now());
                    self.process_iq_cmds(vec![cmd]);
                }
            }
            let has_iq_file = !self.chrome.iq.playback_path.trim().is_empty();
            let play_resp = crate::status_widgets::iq_playback_chip(
                ui,
                self.engine_ui.stats.iq_playback,
                has_iq_file,
            );
            if play_resp.clicked() {
                if let Some(cmd) = self.chrome.iq.replay_playback() {
                    self.process_iq_cmds(vec![cmd]);
                }
            }
            ui.label(
                egui::RichText::new(format!("{:.0} kS/s", self.engine_ui.stats.effective_sps / 1000.0))
                    .small()
                    .color(MUTED),
            );
            if !self.radio.is_kiwi
                && self.engine_ui.stats.sample_rate > 0.0
                && (self.engine_ui.stats.effective_sps / self.engine_ui.stats.sample_rate) < 0.85
            {
                ui.label(
                    egui::RichText::new(format!(
                        "({:.0} kS/s device)",
                        self.engine_ui.stats.sample_rate / 1000.0
                    ))
                    .small()
                    .color(MUTED),
                );
            }
            if self.engine_ui.stats.iq_playback {
                ui.colored_label(OK, "PLAYBACK");
            }
            if self.engine_ui.stats.dropped > 0 {
                ui.colored_label(WARN, format!("drops {}", self.engine_ui.stats.dropped));
            }
            if streaming && !(self.chrome.show_left && self.chrome.show_smeter) {
                show_status_rf_meter(
                    ui,
                    self.rf_meter_dbm(),
                    self.engine_ui.stats.rssi_dbm,
                );
            }
            if self.connection_unstable() {
                ui.colored_label(WARN, "connection unstable");
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button("?")
                    .on_hover_text("Keyboard shortcuts")
                    .clicked()
                {
                    self.chrome.show_shortcuts = !self.chrome.show_shortcuts;
                }
                if ui
                    .button("F11")
                    .on_hover_text("Toggle fullscreen (F11)")
                    .clicked()
                {
                    let on = ui.input(|i| i.viewport().fullscreen.unwrap_or(false));
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::Fullscreen(!on));
                }
                ui.separator();
                panel_toggle(ui, &mut self.chrome.show_console, "Log", "Application log (`)");
                panel_toggle(ui, &mut self.chrome.show_history, "Spots", "Decoded callsign history");
                if panel_toggle(
                    ui,
                    &mut self.chrome.show_af_scope,
                    "Scope",
                    "AF scope for RF gain tuning (G)",
                ) {
                    self.on_af_scope_panel_changed();
                }
                panel_toggle(
                    ui,
                    &mut self.chrome.show_smeter,
                    "Meter",
                    "S-meter and IF/AF AGC levels",
                );
                panel_toggle(ui, &mut self.chrome.show_right, "DSP", "CW demod, skimmer, audio, display");
                panel_toggle(ui, &mut self.chrome.show_left, "RX", "VFO, RF gains, IQ chain");
            });
        });

        if let Some(err) = &self.engine_ui.last_error {
            if matches!(self.engine_ui.conn_state, ConnState::Reconnecting { .. }) {
                ui.colored_label(WARN, err);
            }
        }
    }


}
