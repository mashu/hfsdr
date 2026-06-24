// `engine` — `WaterfallApp` methods.

    fn start_kiwi_directory_fetch(&mut self, force_refresh: bool) {
        if self.kiwi_directory_rx.is_some() {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        self.kiwi_directory_rx = Some(rx);
        std::thread::spawn(move || {
            let result = if force_refresh {
                crate::kiwi_directory::refresh_nearby_receivers()
            } else {
                crate::kiwi_directory::load_nearby_receivers()
            };
            let _ = tx.send(result);
        });
    }



    fn poll_kiwi_directory(&mut self) {
        let Some(rx) = &self.kiwi_directory_rx else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok((geo, receivers))) => {
                self.kiwi_geo = geo;
                self.kiwi_nearby = receivers;
                self.kiwi_directory_error = None;
                self.kiwi_directory_rx = None;
            }
            Ok(Err(err)) => {
                self.kiwi_directory_error = Some(err);
                self.kiwi_directory_rx = None;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.kiwi_directory_rx = None;
            }
        }
    }



    fn connection_unstable(&self) -> bool {
        self.stats.slow
            || matches!(
                self.conn_state,
                ConnState::Reconnecting { .. } | ConnState::Connecting { .. }
            )
    }



    /// Heavy local IQ rate (demod / ring load), not the decimated spectrum span.
    fn is_wideband_device(&self) -> bool {
        !self.is_kiwi && self.iq_passband_hz() > 96_000.0
    }



    /// Wideband local SDR for UI caps (FPS, channel limits).
    fn is_wideband(&self) -> bool {
        self.is_wideband_device()
    }



    /// Skimmer peak/decoders need a manageable spectrum span (≤96 kHz on Airspy).
    fn skimmer_spectrum_ok(&self) -> bool {
        self.is_kiwi || self.plot_full_span_hz() <= 96_000.0
    }



    fn skimmer_runtime_enabled(&self) -> bool {
        self.skimmer_enabled && self.skimmer_spectrum_ok()
    }



    /// Cap repaint rate on wideband to leave CPU for FFT + texture work.
    fn effective_target_fps(&self) -> u32 {
        if self.is_wideband() {
            self.target_fps.min(15)
        } else if self.skimmer_enabled && self.sample_rate > 24_000.0 {
            self.target_fps.min(30)
        } else {
            self.target_fps
        }
    }



    /// Scale skimmer decoder count with available bandwidth.
    fn effective_skimmer(&self) -> SkimmerConfig {
        let mut cfg = self.skimmer.clone().clamped();
        if self.is_wideband() {
            cfg.max_channels = cfg.max_channels.min(8);
        } else if self.sample_rate > 24_000.0 {
            cfg.max_channels = cfg.max_channels.min(12);
        }
        cfg
    }



    /// Push UI settings to the engine and pull its published rows/status/spots.
    fn pump_engine(&mut self) {
        self.cw.listen_offset_hz = ChannelOffsetHz::new(self.listen_offset_hz() as f32);
        self.plot_view
            .clamp_pan(self.plot_full_span_hz(), self.plot_max_zoom_out());
        self.engine.set_params(EngineParams {
            cw: self.cw.clone(),
            audio_enabled: self.audio_enabled,
            volume: self.volume,
            skimmer_enabled: self.skimmer_runtime_enabled(),
            skimmer: self.effective_skimmer(),
            fft_size: self.fft_size,
            fft_auto: self.fft_auto,
            full_drain_spectrum: self.full_drain_spectrum,
        });

        let Some(poll) = self.engine.try_poll() else {
            return;
        };

        if poll.stats.slow && !self.stats.slow {
            log::warn("link slow or unstable");
        }
        self.conn_state = poll.state;
        self.stats = poll.stats;
        if self.scp_reload_pending {
            if self.stats.scp.loaded {
                let n = self.stats.scp.calls;
                self.scp_notice = Some(format!("MASTER.SCP loaded ({n} calls)"));
                self.scp_reload_pending = false;
                self.scp_reload_deadline = None;
            } else if self.scp_reload_deadline.is_some_and(|t| Instant::now() >= t) {
                self.scp_notice = Some(
                    "MASTER.SCP reload failed — file missing or empty (try Download)".into(),
                );
                self.scp_reload_pending = false;
                self.scp_reload_deadline = None;
            }
        }
        self.last_scp_loaded = self.stats.scp.loaded;
        if poll.last_error.as_deref() != self.last_error.as_deref() {
            if let Some(ref err) = poll.last_error {
                log::error(err);
            }
        }
        self.last_error = poll.last_error;
        self.skimmer_spots = poll.spots;
        self.audio_scope = poll.audio_scope;
        let latest = poll.latest;
        let new_rows = poll.rows;
        if matches!(self.conn_state, ConnState::Streaming)
            && self.waterfall_viewport_texture.is_none()
            && self.rows.is_empty()
            && !new_rows.is_empty()
        {
            self.force_texture_full = true;
            self.textures_dirty = true;
        }
        if latest.len() != self.latest.len() {
            // FFT size changed under us: adopt the new width and reset buffers.
            self.latest = latest;
            self.rows.clear();
            self.force_texture_full = true;
            self.textures_dirty = true;
        } else {
            self.latest.copy_from_slice(&latest);
            self.latest_frame_tick = true;
        }

        self.sample_rate = self.stats.sample_rate;
        self.is_kiwi = self.stats.is_kiwi;
        if self.stats.kiwi_has_rf_attn && !self.last_kiwi_has_rf_attn {
            self.apply_kiwi_rf_attn_settings();
        }
        self.last_snr_db = self.stats.snr_db;
        self.skimmer_channels = self.stats.skimmer_channels;
        if self.fft_auto {
            self.fft_size = self.stats.spectrum_fft.max(1024);
        }

        let full_span = self.plot_full_span_hz();
        let max_zoom = self.plot_max_zoom_out();
        let _view_span = self.plot_view.view_span_hz(full_span, max_zoom);
        let _view_pan = self.plot_view.pan_offset_hz;

        if !new_rows.is_empty() {
            let n_new = new_rows.len();
            for row in new_rows {
                let mut stored = if self.rows.len() >= WATERFALL_ROWS {
                    self.rows.pop_back().unwrap_or_else(|| vec![-120.0; row.len()])
                } else {
                    vec![-120.0; row.len()]
                };
                if stored.len() != row.len() {
                    stored.resize(row.len(), -120.0);
                }
                stored.copy_from_slice(&row);
                self.rows.push_front(stored);
            }
            self.waterfall_rows = self.rows.len();
            self.pending_row_appends += n_new;
            self.pending_viewport_row_appends += n_new;
            self.textures_dirty = true;
            let levels_due = self
                .last_display_levels_at
                .map(|t| t.elapsed() >= Duration::from_millis(300))
                .unwrap_or(true);
            if levels_due {
                self.update_display_levels();
                self.last_display_levels_at = Some(Instant::now());
            }
        }

        self.apply_pitch_lock();
        if self.skimmer_enabled {
            self.annotate_new_spots(self.center_khz * 1000.0);
        }
    }

