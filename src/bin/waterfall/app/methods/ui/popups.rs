// `ui/popups` — `WaterfallApp` methods.

    fn connection_popup(&mut self, ctx: &egui::Context) {
        if !self.show_connection_drawer {
            return;
        }
        let screen = ctx.content_rect();
        let max_h = (screen.height() - 12.0).max(320.0);
        let win_h = (screen.height() * 0.86).clamp(420.0, max_h);
        let mut open = self.show_connection_drawer;
        let (status_label, status_color) = self.connection_status_pill();
        configure_popup_window(
            "connection_popup",
            [screen.left() + 12.0, screen.top() + 36.0],
            500.0,
            win_h,
            280.0,
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
            popup_scroll_body(ui, |ui| {
                self.connection_card(ui);
            });
        });
        self.show_connection_drawer = open;
    }



    fn iq_popup(&mut self, ctx: &egui::Context) {
        if !self.show_iq_drawer {
            return;
        }
        let screen = ctx.content_rect();
        let max_h = (screen.height() - 12.0).max(200.0);
        let win_h = 300.0_f32.clamp(200.0, max_h);
        let mut open = self.show_iq_drawer;
        let subtitle = format!(
            "{:.0}% · {:.2}s queued",
            self.stats.iq_buffer_fill * 100.0,
            self.stats.iq_buffer_secs,
        );
        let status = if self.stats.iq_recording {
            let secs =
                self.stats.iq_capture_samples as f32 / self.stats.sample_rate.max(1.0);
            Some((format!("REC {secs:.0}s"), WARN))
        } else if self.stats.iq_playback {
            Some(("PLAYBACK".to_string(), OK))
        } else {
            None
        };
        configure_popup_window(
            "iq_popup",
            [screen.left() + 200.0, screen.top() + 36.0],
            420.0,
            win_h,
            200.0,
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
            popup_scroll_body(ui, |ui| {
                let streaming = matches!(self.conn_state, ConnState::Streaming);
                let (cmds, dirty) = self.iq.show(
                    ui,
                    IqPanelView {
                        stats: &self.stats,
                        streaming,
                    },
                );
                if dirty {
                    self.settings_dirty_at = Some(Instant::now());
                }
                self.process_iq_cmds(cmds);
            });
        });
        self.show_iq_drawer = open;
    }



    fn pipeline_popup(&mut self, ctx: &egui::Context) {
        if !self.show_pipeline_drawer {
            return;
        }
        let screen = ctx.content_rect();
        let max_h = (screen.height() - 12.0).max(320.0);
        let win_h = 640.0_f32.clamp(420.0, max_h);
        let mut open = self.show_pipeline_drawer;
        let streaming = matches!(self.conn_state, ConnState::Streaming);
        let subtitle = format!(
            "{:.0} kS/s · {} IQ",
            self.stats.effective_sps / 1000.0,
            if streaming { "live" } else { "idle" },
        );
        let status = if self.stats.slow {
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
            win_h,
            420.0,
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
            popup_scroll_body(ui, |ui| {
                let snap = PipelineSnapshot {
                    source_label: &self.connection_alias(),
                    streaming,
                    device_rate_hz: self.stats.sample_rate.max(self.form_sample_rate as f32),
                    ingress_decim: self.pipeline_ingress_decim(),
                    cw: &self.cw,
                    skimmer_enabled: self.skimmer_enabled,
                    audio_enabled: self.audio_enabled,
                    stats: &self.stats,
                };
                let toggled = self.pipeline_flow.show(ui, &snap);
                for stage in toggled {
                    self.toggle_pipeline_stage(stage);
                }
            });
        });
        self.show_pipeline_drawer = open;
    }



    fn shortcuts_popup(&mut self, ctx: &egui::Context) {
        if !self.show_shortcuts {
            return;
        }
        let mut open = self.show_shortcuts;
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
                    ("← / →", "Pan view (zoomed) or tune RX · hold to accelerate"),
                    ("Shift / Ctrl", "Fine / fast pan steps"),
                    ("Z", "Zero-beat to strongest carrier"),
                    ("L", "Lock pitch to BFO"),
                    (", / .", "RIT −10 / +10 Hz"),
                    ("\\", "Clear RIT"),
                    ("[ / ]", "Narrow / widen filter"),
                    ("1 – 4", "Toggle IQ notches"),
                    ("N / B / R / A / P", "Auto-notch / blanker / NR / AGC / APF"),
                    ("G", "Toggle AF tuning scope (RF gain aid)"),
                    ("F / M", "Full IQ span / band overview"),
                    ("Space / - / +", "Mute / volume down / up"),
                    ("`", "Toggle log panel"),
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
        self.show_shortcuts = open;
    }

