use crate::app::WaterfallApp;
use crate::app::prelude::*;

impl WaterfallApp {

    pub(crate) fn connection_popup(&mut self, ctx: &egui::Context) {
        if !self.connection.form.show_connection_drawer {
            return;
        }
        let screen = ctx.content_rect();
        let max_h = (screen.height() - 12.0).max(320.0);
        let body_max_h = popup_body_max_height(max_h);
        let mut open = self.connection.form.show_connection_drawer;
        let (status_label, status_color) = self.connection_status_pill();
        configure_popup_window(
            "connection_popup",
            [screen.left() + 12.0, screen.top() + 36.0],
            500.0,
            max_h,
        )
        .show(ctx, |ui| {
            popup_header(
                ui,
                PopupHeader {
                    title: "Connection",
                    subtitle: None,
                    status: Some((status_label, status_color)),
                },
                &mut open,
            );
            popup_scroll_body(ui, body_max_h, |ui| {
                self.connection_card(ui);
            });
        });
        self.connection.form.show_connection_drawer = open;
    }



    pub(crate) fn iq_popup(&mut self, ctx: &egui::Context) {
        if !self.chrome.show_iq_drawer {
            return;
        }
        let screen = ctx.content_rect();
        let max_h = (screen.height() - 12.0).max(200.0);
        let body_max_h = popup_body_max_height(max_h);
        let mut open = self.chrome.show_iq_drawer;
        let subtitle = format!(
            "{:.0}% · {:.2}s queued",
            self.engine_ui.stats.iq_buffer_fill * 100.0,
            self.engine_ui.stats.iq_buffer_secs,
        );
        let status = if self.engine_ui.stats.iq_recording {
            let secs =
                self.engine_ui.stats.iq_capture_samples as f32 / self.engine_ui.stats.sample_rate.max(1.0);
            Some((format!("REC {secs:.0}s"), WARN))
        } else if self.engine_ui.stats.iq_playback {
            Some(("PLAYBACK".to_string(), OK))
        } else {
            None
        };
        configure_popup_window(
            "iq_popup",
            [screen.left() + 200.0, screen.top() + 36.0],
            420.0,
            max_h,
        )
        .show(ctx, |ui| {
            popup_header(
                ui,
                PopupHeader {
                    title: "IQ I/O",
                    subtitle: Some(&subtitle),
                    status,
                },
                &mut open,
            );
            popup_scroll_body(ui, body_max_h, |ui| {
                let streaming = matches!(self.engine_ui.conn_state, ConnState::Streaming);
                let (cmds, dirty) = self.chrome.iq.show(
                    ui,
                    IqPanelView {
                        stats: &self.engine_ui.stats,
                        streaming,
                    },
                );
                if dirty {
                    self.settings_dirty_at = Some(Instant::now());
                }
                self.process_iq_cmds(cmds);
            });
        });
        self.chrome.show_iq_drawer = open;
    }



    pub(crate) fn pipeline_popup(&mut self, ctx: &egui::Context) {
        if !self.chrome.show_pipeline_drawer {
            return;
        }
        let screen = ctx.content_rect();
        let max_h = (screen.height() - 12.0).max(320.0);
        let body_max_h = popup_body_max_height(max_h);
        let mut open = self.chrome.show_pipeline_drawer;
        let streaming = matches!(self.engine_ui.conn_state, ConnState::Streaming);
        let subtitle = format!(
            "{:.0} kS/s · {} IQ",
            self.engine_ui.stats.effective_sps / 1000.0,
            if streaming { "live" } else { "idle" },
        );
        let status = if self.engine_ui.stats.slow {
            Some(("SLOW".to_string(), WARN))
        } else if streaming {
            Some(("LIVE".to_string(), OK))
        } else {
            None
        };
        configure_popup_window(
            "pipeline_popup",
            [
                screen.left() + (screen.width() - 860.0) * 0.5,
                screen.top() + 36.0,
            ],
            860.0,
            max_h,
        )
        .show(ctx, |ui| {
            popup_header(
                ui,
                PopupHeader {
                    title: "Receive pipeline",
                    subtitle: Some(&subtitle),
                    status,
                },
                &mut open,
            );
            popup_scroll_body(ui, body_max_h, |ui| {
                let snap = PipelineSnapshot {
                    source_label: &self.connection_alias(),
                    streaming,
                    device_rate_hz: self.engine_ui.stats.sample_rate.max(self.connection.form.sample_rate as f32),
                    ingress_decim: self.pipeline_ingress_decim(),
                    cw: &self.radio.cw,
                    skimmer_enabled: self.skimmer_ui.skimmer_enabled,
                    audio_enabled: self.audio.audio_enabled,
                    rf_gain_db: self.radio.rf_gain_db,
                    stats: &self.engine_ui.stats,
                };
                let toggled = self.chrome.pipeline_flow.show(ui, &snap);
                for stage in toggled {
                    self.toggle_pipeline_stage(stage);
                }
            });
        });
        self.chrome.show_pipeline_drawer = open;
    }



    pub(crate) fn filter_popup(&mut self, ctx: &egui::Context) {
        if !self.chrome.show_filter_drawer {
            return;
        }
        let screen = ctx.content_rect();
        let max_h = (screen.height() - 12.0).max(280.0);
        let body_max_h = popup_body_max_height(max_h);
        let mut open = self.chrome.show_filter_drawer;
        let audio_rate = hfsdr::audio_sample_rate(
            self.radio.sample_rate.max(self.engine_ui.stats.sample_rate),
            self.radio.cw.decimation,
        );
        let channel_half_hz = self.filter_overlay_cached().channel_half_hz;
        let span_hz = hfsdr::filter_curve_span_hz(self.radio.cw.passband_hz, channel_half_hz);
        let spectrum = self.spectrum_view();
        let streaming = matches!(self.engine_ui.conn_state, ConnState::Streaming);
        let listen_offset_hz = self.listen_offset_hz();
        let filter_shift_hz = self.radio.cw.filter_shift_hz.hz() as f64;
        let ref_db = self.display.ref_db;
        let range_db = self.display.range_db;
        let trace_db = self.plot.smoothed_trace.clone();
        let cw_settings = self.radio.cw.clone();
        let channel_bypass = self.radio.cw.diagnostic.channel_fir;
        let subtitle = format!(
            "{:.0} Hz BW · {} notches · {:.0} Hz audio",
            self.radio.cw.passband_hz,
            self.radio.cw.notches.iter().filter(|n| n.enabled).count(),
            audio_rate,
        );
        configure_popup_window(
            "filter_popup",
            [
                screen.left() + (screen.width() - 520.0) * 0.5,
                screen.top() + 48.0,
            ],
            520.0,
            max_h,
        )
        .show(ctx, |ui| {
            popup_header(
                ui,
                PopupHeader {
                    title: "Filter response",
                    subtitle: Some(&subtitle),
                    status: None,
                },
                &mut open,
            );
            popup_scroll_body(ui, body_max_h, |ui| {
                crate::filter_diagnostic::show_filter_diagnostic_panel(
                    ui,
                    &mut self.chrome.filter_diagnostic,
                    &crate::filter_diagnostic::FilterDiagnosticView {
                        settings: &cw_settings,
                        audio_rate,
                        span_hz,
                        channel_half_hz,
                        channel_bypass,
                        trace_db: &trace_db,
                        trace_view_span_hz: spectrum.view_span_hz,
                        trace_pan_hz: spectrum.compose_pan_offset_hz,
                        listen_offset_hz,
                        filter_shift_hz,
                        ref_db,
                        range_db,
                        streaming,
                    },
                );
            });
        });
        self.chrome.show_filter_drawer = open;
    }



    pub(crate) fn envelope_popup(&mut self, ctx: &egui::Context) {
        if !self.chrome.show_envelope_drawer {
            return;
        }
        let screen = ctx.content_rect();
        let max_h = (screen.height() - 12.0).max(280.0);
        let body_max_h = popup_body_max_height(max_h);
        let mut open = self.chrome.show_envelope_drawer;
        let audio_rate = hfsdr::audio_sample_rate(
            self.radio.sample_rate.max(self.engine_ui.stats.sample_rate),
            self.radio.cw.decimation,
        );
        let streaming = matches!(self.engine_ui.conn_state, ConnState::Streaming);
        let st = self.radio.cw.sidetone_envelope;
        let subtitle = if st.enabled {
            format!(
                "rise {:.1} ms · fall {:.1} ms · {:.0} Hz audio",
                st.rise_ms,
                st.fall_ms,
                audio_rate,
            )
        } else {
            format!("Off · {:.0} Hz audio", audio_rate)
        };
        configure_popup_window(
            "envelope_popup",
            [
                screen.left() + (screen.width() - 520.0) * 0.5,
                screen.top() + 48.0,
            ],
            520.0,
            max_h,
        )
        .show(ctx, |ui| {
            popup_header(
                ui,
                PopupHeader {
                    title: "Sidetone envelope",
                    subtitle: Some(&subtitle),
                    status: None,
                },
                &mut open,
            );
            popup_scroll_body(ui, body_max_h, |ui| {
                crate::envelope_diagnostic::show_envelope_diagnostic_panel(
                    ui,
                    &crate::envelope_diagnostic::EnvelopeDiagnosticView {
                        settings: &self.radio.cw.sidetone_envelope,
                        audio_rate,
                        streaming,
                    },
                );
            });
        });
        self.chrome.show_envelope_drawer = open;
    }



    pub(crate) fn shortcuts_popup(&mut self, ctx: &egui::Context) {
        if !self.chrome.show_shortcuts {
            return;
        }
        let mut open = self.chrome.show_shortcuts;
        egui::Window::new("Keyboard shortcuts")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .frame(crate::popup::popup_window_frame())
            .show(ctx, |ui| {
                ui.set_max_width(420.0);
                ui.label(
                    egui::RichText::new("Press ? again to close")
                        .small()
                        .color(MUTED),
                );
                ui.add_space(6.0);
                for (keys, action) in [
                    ("Ctrl+drag", "Cyan band = shift filter · edges = BW · purple notches"),
                    ("Click", "Tune VFO · drag = walk frequency (Shift+drag = pan when zoomed)"),
                    ("Shift / Ctrl", "Fine / fast pan steps"),
                    ("Z", "Zero-beat to strongest carrier"),
                    ("L", "Lock pitch to BFO"),
                    ("R", "Toggle RIT — offset receive without changing RX MHz"),
                    (", / .", "RIT −10 / +10 Hz"),
                    ("\\", "Clear RIT"),
                    ("[ / ]", "Narrow / widen filter"),
                    ("1 – 4", "Toggle IQ notches"),
                    ("N / B / A / P", "Auto-notch / blanker / AGC / APF"),
                    ("G", "Toggle AF tuning scope (RF gain aid)"),
                    ("F / M", "Full IQ span / band overview"),
                    ("Space / - / +", "Mute / volume down / up"),
                    ("`", "Toggle log panel"),
                    ("Enter", "Quick connect to last receiver"),
                    ("Esc", "Close panel · disconnect · quit"),
                    ("F11", "Fullscreen"),
                ] {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(keys)
                                .monospace()
                                .color(ACCENT)
                                .size(12.0),
                        );
                        ui.label(egui::RichText::new(action).small());
                    });
                }
                ui.add_space(8.0);
                if ui.button("Close").clicked() {
                    open = false;
                }
            });
        self.chrome.show_shortcuts = open;
    }


}
