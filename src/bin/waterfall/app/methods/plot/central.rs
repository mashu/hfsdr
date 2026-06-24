// `plot/central` — `WaterfallApp` methods.

    fn central_panel(&mut self, ui: &mut egui::Ui) {
        if !matches!(self.conn_state, ConnState::Streaming) {
            ui.horizontal_wrapped(|ui| {
                match &self.conn_state {
                    ConnState::Reconnecting { attempt, retry_in_s } => {
                        ui.colored_label(
                            WARN,
                            format!(
                                "Reconnecting (attempt {attempt}) in {retry_in_s:.0}s — keeping last spectrum"
                            ),
                        );
                    }
                    ConnState::Connecting { label } => {
                        ui.colored_label(WARN, format!("Connecting to {label}…"));
                    }
                    ConnState::Disconnected => {
                        ui.colored_label(
                            MUTED,
                            "Not connected — click OFFLINE in the status bar or ⚡ to connect",
                        );
                    }
                    ConnState::Streaming => {}
                }
            });
            ui.add_space(4.0);
        }

        self.plot_view
            .clamp_pan(self.plot_full_span_hz(), self.plot_max_zoom_out());
        let view = self.spectrum_view();
        let plot_full_span = self.plot_full_span_hz();
        let max_zoom = self.plot_max_zoom_out();
        update_trace(
            &self.latest,
            &mut self.smoothed_trace,
            &mut self.trace_composed,
            &mut self.trace_view_key,
            view.row_rate_hz,
            view.view_span_hz,
            view.data_span_hz,
            view.compose_pan_offset_hz,
            view.allow_band_padding,
            self.smooth_alpha,
            self.latest_frame_tick,
        );
        if self.show_band_overview && self.is_kiwi {
            update_trace(
                &self.latest,
                &mut self.overview_smoothed,
                &mut self.overview_composed,
                &mut self.overview_view_key,
                self.sample_rate,
                plot_full_span,
                plot_full_span,
                0.0,
                true,
                self.smooth_alpha,
                self.latest_frame_tick,
            );
        }
        let overview_span_hz = self.band_overview_span_hz();

        let tune_preview_offset_hz = self.tune_preview_offset_hz.unwrap_or(0.0);
        let listen_center_hz = self.listen_offset_hz();
        let notches = self.enabled_notches();
        let labels = if self.skimmer_enabled {
            self.spot_labels(self.center_khz * 1000.0)
        } else {
            Vec::new()
        };

        let bw_max = self.passband_max_hz();
        let plot_width = ui.available_width().round() as usize;
        self.sync_waterfall_storage(ui.ctx());
        self.sync_waterfall_viewport(ui.ctx(), plot_width);
        let storage_span = self.waterfall_storage_view().view_span_hz;
        let freq_map = PlotFreqMapping::new(
            view.view_span_hz,
            view.pan_offset_hz,
            storage_span,
        );
        let params = crate::widgets::PlotParams {
            view_bandwidth_hz: plot_full_span,
            max_zoom,
            center_freq_hz: self.center_khz * 1000.0,
            passband_hz: self.cw.passband_hz,
            passband_min_hz: CW_PASSBAND_MIN_HZ,
            passband_max_hz: bw_max,
            filter_editable: true,
            listen_center_hz,
            tune_preview_offset_hz,
            notches: &notches,
            labels: &labels,
            trace: &self.smoothed_trace,
            overview_trace: if self.show_band_overview && self.is_kiwi {
                &self.overview_smoothed
            } else {
                &[]
            },
            overview_span_hz,
            show_overview: self.show_band_overview && self.is_kiwi,
            ref_db: self.ref_db,
            range_db: self.range_db,
            height: SCOPE_HEIGHT,
            plot_width: plot_width as f32,
            waterfall_display: self.waterfall_viewport_texture.as_ref(),
        };

        let plot_actions = self.panadapter_plot.show(
            ui,
            &mut self.plot_interaction,
            &mut self.plot_view,
            freq_map,
            &params,
            &mut self.hover_offset_hz,
            &mut self.last_plot_interaction_rect,
        );

        let view_dirty = plot_actions.iter().any(plot_action_changes_view);
        self.apply_plot_actions(plot_actions);
        if view_dirty {
            self.refresh_plot_composites(ui.ctx(), plot_width);
            ui.ctx().request_repaint();
        }
    }





    fn refresh_plot_composites(&mut self, ctx: &egui::Context, plot_width: usize) {
        let view = self.spectrum_view();
        let plot_full_span = self.plot_full_span_hz();
        update_trace(
            &self.latest,
            &mut self.smoothed_trace,
            &mut self.trace_composed,
            &mut self.trace_view_key,
            view.row_rate_hz,
            view.view_span_hz,
            view.data_span_hz,
            view.compose_pan_offset_hz,
            view.allow_band_padding,
            self.smooth_alpha,
            true,
        );
        if self.show_band_overview && self.is_kiwi {
            update_trace(
                &self.latest,
                &mut self.overview_smoothed,
                &mut self.overview_composed,
                &mut self.overview_view_key,
                self.sample_rate,
                plot_full_span,
                plot_full_span,
                0.0,
                true,
                self.smooth_alpha,
                true,
            );
        }
        self.sync_waterfall_viewport(ctx, plot_width);
    }
