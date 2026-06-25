use crate::app::WaterfallApp;
use crate::app::prelude::*;

impl WaterfallApp {

    pub(crate) fn start_kiwi_directory_fetch(&mut self, force_refresh: bool) {
        if self.connection.kiwi.fetch_rx.is_some() {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        self.connection.kiwi.fetch_rx = Some(rx);
        std::thread::spawn(move || {
            let result = if force_refresh {
                crate::kiwi_directory::refresh_nearby_receivers()
            } else {
                crate::kiwi_directory::load_nearby_receivers()
            };
            let _ = tx.send(result);
        });
    }



    pub(crate) fn poll_kiwi_directory(&mut self) {
        let Some(rx) = &self.connection.kiwi.fetch_rx else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok((geo, receivers))) => {
                self.connection.kiwi.geo = geo;
                self.connection.kiwi.nearby = receivers;
                self.connection.kiwi.error = None;
                self.connection.kiwi.fetch_rx = None;
            }
            Ok(Err(err)) => {
                self.connection.kiwi.error = Some(err);
                self.connection.kiwi.fetch_rx = None;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.connection.kiwi.fetch_rx = None;
            }
        }
    }



    pub(crate) fn connection_unstable(&self) -> bool {
        self.engine_ui.stats.slow
            || matches!(
                self.engine_ui.conn_state,
                ConnState::Reconnecting { .. } | ConnState::Connecting { .. }
            )
    }



    /// Heavy local IQ rate (demod / ring load), not the decimated spectrum span.
    pub(crate) fn is_wideband_device(&self) -> bool {
        !self.radio.is_kiwi && self.iq_passband_hz() > 96_000.0
    }



    /// Wideband local SDR for UI caps (FPS, channel limits).
    pub(crate) fn is_wideband(&self) -> bool {
        self.is_wideband_device()
    }



    /// Skimmer peak/decoders need a manageable spectrum span (≤96 kHz on Airspy).
    pub(crate) fn skimmer_spectrum_ok(&self) -> bool {
        self.radio.is_kiwi || self.plot_full_span_hz() <= 96_000.0
    }



    pub(crate) fn skimmer_runtime_enabled(&self) -> bool {
        self.skimmer_ui.skimmer_enabled && self.skimmer_spectrum_ok()
    }



    /// Cap repaint rate on wideband to leave CPU for FFT + texture work.
    pub(crate) fn effective_target_fps(&self) -> u32 {
        if self.is_wideband() {
            self.display.target_fps.min(15)
        } else if self.skimmer_ui.skimmer_enabled && self.radio.sample_rate > 24_000.0 {
            self.display.target_fps.min(30)
        } else {
            self.display.target_fps
        }
    }



    /// Scale skimmer decoder count with available bandwidth.
    pub(crate) fn effective_skimmer(&self) -> SkimmerConfig {
        let mut cfg = self.skimmer_ui.skimmer.clone().clamped();
        if matches!(self.engine_ui.conn_state, ConnState::Streaming) {
            cfg.source_label = self.connection_alias();
        }
        if self.is_wideband() {
            cfg.max_channels = cfg.max_channels.min(8);
        } else if self.radio.sample_rate > 24_000.0 {
            cfg.max_channels = cfg.max_channels.min(12);
        }
        cfg
    }



    /// Push UI settings to the engine and pull its published rows/status/spots.
    pub(crate) fn pump_engine(&mut self) {
        self.radio.cw.listen_offset_hz = ChannelOffsetHz::new(self.listen_offset_hz() as f32);
        self.plot.plot_view
            .clamp_pan(self.plot_full_span_hz(), self.plot_max_zoom_out());
        self.engine.set_params(EngineParams {
            cw: self.radio.cw.clone(),
            audio_enabled: self.audio.audio_enabled,
            volume: self.audio.volume,
            skimmer_enabled: self.skimmer_runtime_enabled(),
            skimmer: self.effective_skimmer(),
            fft_size: self.display.fft_size,
            fft_auto: self.display.fft_auto,
            full_drain_spectrum: self.display.full_drain_spectrum,
            rf_gain_db: self.radio.rf_gain_db,
            perf_trace: self.display.perf_trace,
        });

        let Some(poll) = self.engine.try_poll() else {
            return;
        };
        let poll = poll.sanitized(self.plot.latest.len().max(FFT_SIZE));

        if poll.stats.slow && !self.engine_ui.stats.slow {
            log::warn("link slow or unstable");
        }
        self.engine_ui.conn_state = poll.state;
        self.engine_ui.stats = poll.stats;
        if self.skimmer_ui.scp_reload_pending {
            if self.engine_ui.stats.scp.loaded {
                let n = self.engine_ui.stats.scp.calls;
                self.skimmer_ui.scp_notice = Some(format!("MASTER.SCP loaded ({n} calls)"));
                self.skimmer_ui.scp_reload_pending = false;
                self.skimmer_ui.scp_reload_deadline = None;
            } else if self.skimmer_ui.scp_reload_deadline.is_some_and(|t| Instant::now() >= t) {
                self.skimmer_ui.scp_notice = Some(
                    "MASTER.SCP reload failed — file missing or empty (try Download)".into(),
                );
                self.skimmer_ui.scp_reload_pending = false;
                self.skimmer_ui.scp_reload_deadline = None;
            }
        }
        self.skimmer_ui.last_scp_loaded = self.engine_ui.stats.scp.loaded;
        if poll.last_error.as_deref() != self.engine_ui.last_error.as_deref() {
            if let Some(ref err) = poll.last_error {
                log::error(err);
            }
        }
        self.engine_ui.last_error = poll.last_error;
        self.skimmer_ui.skimmer_spots = poll.spots;
        self.audio.audio_scope = poll.audio_scope;
        let latest = poll.latest;
        let new_rows = poll.rows;
        if matches!(self.engine_ui.conn_state, ConnState::Streaming)
            && self.plot.waterfall.viewport_texture.is_none()
            && self.plot.rows.is_empty()
            && !new_rows.is_empty()
        {
            self.plot.waterfall.force_texture_full = true;
            self.plot.waterfall.textures_dirty = true;
        }
        if latest.len() != self.plot.latest.len() {
            // FFT size changed under us: adopt the new width and reset buffers.
            self.plot.latest = latest;
            self.plot.rows.clear();
            self.plot.waterfall.force_texture_full = true;
            self.plot.waterfall.textures_dirty = true;
        } else {
            self.plot.latest.copy_from_slice(&latest);
            self.plot.latest_frame_tick = true;
        }

        self.radio.sample_rate = self.engine_ui.stats.sample_rate;
        self.radio.is_kiwi = self.engine_ui.stats.is_kiwi;
        if self.engine_ui.stats.kiwi_has_rf_attn && !self.radio.last_kiwi_has_rf_attn {
            self.apply_kiwi_rf_attn_settings();
        }
        self.radio.last_snr_db = self.engine_ui.stats.snr_db;
        self.skimmer_ui.skimmer_channels = self.engine_ui.stats.skimmer_channels;
        if self.display.fft_auto {
            self.display.fft_size = self.engine_ui.stats.spectrum_fft.max(1024);
        }

        let full_span = self.plot_full_span_hz();
        let max_zoom = self.plot_max_zoom_out();
        let _view_span = self.plot.plot_view.view_span_hz(full_span, max_zoom);
        let _view_pan = self.plot.plot_view.pan_offset_hz;

        if !new_rows.is_empty() {
            let n_new = new_rows.len();
            for row in new_rows {
                let mut stored = if self.plot.rows.len() >= WATERFALL_ROWS {
                    self.plot.rows.pop_back().unwrap_or_else(|| vec![-120.0; row.len()])
                } else {
                    vec![-120.0; row.len()]
                };
                if stored.len() != row.len() {
                    stored.resize(row.len(), -120.0);
                }
                stored.copy_from_slice(&row);
                self.plot.rows.push_front(stored);
            }
            self.display.waterfall_rows = self.plot.rows.len();
            self.plot.waterfall.pending_row_appends += n_new;
            self.plot.waterfall.pending_viewport_row_appends += n_new;
            self.plot.waterfall.textures_dirty = true;
            let levels_due = self
                .plot.last_display_levels_at
                .map(|t| t.elapsed() >= Duration::from_millis(300))
                .unwrap_or(true);
            if levels_due {
                self.update_display_levels();
                self.plot.last_display_levels_at = Some(Instant::now());
            }
        }

        self.apply_pitch_lock();
        if self.skimmer_ui.skimmer_enabled {
            self.annotate_new_spots(self.radio.center_khz * 1000.0);
        }
    }


}
