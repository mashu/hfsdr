// `settings` — `WaterfallApp` methods.

    fn apply_settings(&mut self, s: &AppSettings) {
        self.cw.bfo_hz = s.bfo_hz;
        self.cw.passband_hz = s.passband_hz;
        self.cw.channel_filter = channel_filter_from_u8(s.channel_filter);
        self.cw.decim_filter = channel_filter_from_u8(s.decim_filter);
        self.cw.window = window_from_u8(s.window);
        self.cw.kaiser_beta = s.kaiser_beta.clamp(2.0, 14.0);
        self.cw.passband_flatten = s.passband_flatten;
        self.cw.decimation = s.decimation;
        self.cw.noise_blanker.enabled = s.nb_enabled;
        self.cw.noise_blanker.threshold = s.nb_threshold;
        self.cw.noise_blanker.width = s.nb_width as usize;
        self.cw.auto_notch.enabled = s.an_enabled;
        self.cw.auto_notch.guard_hz = s.an_guard_hz;
        self.cw.auto_notch.rate = s.an_rate;
        self.cw.apf.enabled = s.apf_enabled;
        self.cw.apf.width_hz = s.apf_width_hz;
        self.cw.apf.gain = s.apf_gain;
        self.cw.noise_reduction.enabled = s.nr_enabled;
        self.cw.noise_reduction.level = s.nr_level;
        self.cw.agc.enabled = s.agc_enabled;
        self.cw.agc.target = s.agc_target;
        self.cw.agc.attack_ms = s.agc_attack_ms;
        self.cw.agc.decay_ms = s.agc_decay_ms;
        self.cw.agc.manual_gain = s.agc_manual_gain;
        self.cw.agc_mode = agc_mode_from_u8(s.agc_mode);
        for (slot, data) in self.cw.notches.iter_mut().zip(s.notches.iter()) {
            slot.enabled = data.enabled;
            slot.offset_hz = ChannelOffsetHz::new(data.offset_hz);
            slot.width_hz = data.width_hz;
        }

        self.rit_hz = s.rit_hz;
        self.pitch_lock = s.pitch_lock;
        self.lock_ham_bands = s.lock_ham_bands;
        self.agc_rf_on = s.agc_rf_on;
        self.last_agc_rf_on = s.agc_rf_on;

        self.ref_db = s.ref_db;
        self.range_db = s.range_db;
        self.display_auto_track = s.display_auto_track;
        self.show_band_overview = s.show_band_overview;
        self.pan_step_hz = s.pan_step_hz.clamp(10.0, 50_000.0);
        self.pan_step_fast_hz = s.pan_step_fast_hz.clamp(50.0, 500_000.0);
        if self.display_auto_track {
            self.display_levels_initialized = false;
        } else {
            self.display_levels_initialized =
                display_levels_initialized_after_settings_load(self.display_auto_track);
        }
        self.smooth_alpha = s.smooth_alpha;
        self.waterfall_avg = normalize_waterfall_avg(s.waterfall_avg);
        self.target_fps = s.target_fps.clamp(10, 60);
        self.fft_size = s.fft_size.clamp(1024, 65_536);
        self.fft_auto = s.fft_auto;
        self.full_drain_spectrum = s.full_drain_spectrum;

        self.audio_enabled = s.audio_enabled;
        self.volume = s.volume;

        self.skimmer_enabled = s.skimmer_enabled;
        self.skimmer = skimmer_config_from_settings(s);
        self.min_spot_snr = s.min_spot_snr;
        self.spot_cq_only = s.spot_cq_only;
        self.spot_hide_heard_labels = s.spot_hide_heard_labels;
        self.spot_max_age_secs = s.spot_max_age_secs.max(0.0);
        self.spot_callsign_filter = s.spot_callsign_filter.clone();
        self.spot_label_limit = s.spot_label_limit.clamp(8, 80);
        self.spot_sort = spot_sort_from_u8(s.spot_sort);
        self.continent_filter = s.continent_filter;
        self.show_continents = s.show_continents;
        self.show_console = s.show_console;
        self.filter_wide = s.filter_wide;
        if !self.filter_wide && self.cw.passband_hz > CW_PASSBAND_NARROW_MAX_HZ {
            self.cw.passband_hz = CW_PASSBAND_NARROW_MAX_HZ;
        }
        self.show_history = s.show_history;
        self.show_left = s.show_left;
        self.show_right = s.show_right;
        self.show_af_scope = s.show_af_scope;
        self.show_smeter = s.show_smeter;

        self.recent_hosts = s.recent_hosts.clone();
        self.form_kiwi = s.kiwi.clone();
        self.form_kiwi.man_gain = s.kiwi_man_gain;
        self.last_kiwi_man_gain = s.kiwi_man_gain;
        self.last_kiwi_rf_attn_db = self.form_kiwi.rf_attn_db;
        self.form_airspy = s.airspy.clone();
        self.form_rtlsdr = s.rtlsdr.clone();
        self.form_qmx = s.qmx.clone();
        if s.airspy_sample_rate != 0 {
            self.form_sample_rate = s.airspy_sample_rate;
        } else if s.rtlsdr_sample_rate != 0 {
            self.form_sample_rate = s.rtlsdr_sample_rate;
        }
        self.center_khz = s.last_center_mhz * 1000.0;
        self.clamp_center_to_ham_bands();
        self.last_center_khz = self.center_khz;
        self.iq.capture_dir = if s.iq_capture_dir.is_empty() {
            hfsdr::default_capture_dir()
        } else {
            std::path::PathBuf::from(&s.iq_capture_dir)
        };
        self.iq.playback_path = s.iq_playback_path.clone();
    }



    fn current_settings(&self) -> AppSettings {
        AppSettings {
            bfo_hz: self.cw.bfo_hz,
            passband_hz: self.cw.passband_hz,
            channel_filter: channel_filter_to_u8(self.cw.channel_filter),
            decim_filter: channel_filter_to_u8(self.cw.decim_filter),
            window: window_to_u8(self.cw.window),
            kaiser_beta: self.cw.kaiser_beta,
            passband_flatten: self.cw.passband_flatten,
            decimation: self.cw.decimation,
            nb_enabled: self.cw.noise_blanker.enabled,
            nb_threshold: self.cw.noise_blanker.threshold,
            nb_width: self.cw.noise_blanker.width as u32,
            an_enabled: self.cw.auto_notch.enabled,
            an_guard_hz: self.cw.auto_notch.guard_hz,
            an_rate: self.cw.auto_notch.rate,
            apf_enabled: self.cw.apf.enabled,
            apf_width_hz: self.cw.apf.width_hz,
            apf_gain: self.cw.apf.gain,
            nr_enabled: self.cw.noise_reduction.enabled,
            nr_level: self.cw.noise_reduction.level,
            agc_enabled: self.cw.agc.enabled,
            agc_target: self.cw.agc.target,
            agc_attack_ms: self.cw.agc.attack_ms,
            agc_decay_ms: self.cw.agc.decay_ms,
            agc_manual_gain: self.cw.agc.manual_gain,
            agc_mode: agc_mode_to_u8(self.cw.agc_mode),
            notches: self
                .cw
                .notches
                .iter()
                .map(|n| NotchData {
                    enabled: n.enabled,
                    offset_hz: n.offset_hz.hz(),
                    width_hz: n.width_hz,
                })
                .collect(),
            rit_hz: self.rit_hz,
            pitch_lock: self.pitch_lock,
            lock_ham_bands: self.lock_ham_bands,
            agc_rf_on: self.agc_rf_on,
            kiwi_man_gain: self.form_kiwi.man_gain,
            ref_db: self.ref_db,
            range_db: self.range_db,
            display_auto_track: self.display_auto_track,
            show_band_overview: self.show_band_overview,
            pan_step_hz: self.pan_step_hz,
            pan_step_fast_hz: self.pan_step_fast_hz,
            smooth_alpha: self.smooth_alpha,
            waterfall_avg: self.waterfall_avg,
            target_fps: self.target_fps,
            fft_size: self.fft_size,
            fft_auto: self.fft_auto,
            full_drain_spectrum: self.full_drain_spectrum,
            audio_enabled: self.audio_enabled,
            volume: self.volume,
            skimmer_enabled: self.skimmer_enabled,
            skimmer_min_snr_db: self.skimmer.min_snr_db,
            skimmer_min_decode_snr_db: self.skimmer.min_decode_snr_db,
            skimmer_decode_gate_ms: self.skimmer.decode_gate_ms,
            skimmer_max_channels: self.skimmer.max_channels,
            skimmer_bucket_hz: self.skimmer.bucket_hz,
            skimmer_min_separation_bins: self.skimmer.min_separation_bins,
            skimmer_decoder: skimmer_decoder_to_u8(self.skimmer.decoder),
            skimmer_beam_width: self.skimmer.decoder_params.beam_width,
            skimmer_lpf_cutoff_hz: self.skimmer.lpf_cutoff_hz,
            skimmer_target_audio_rate_hz: self.skimmer.target_audio_rate_hz,
            skimmer_initial_wpm: self.skimmer.decoder_params.initial_wpm,
            skimmer_thr_low: self.skimmer.decoder_params.envelope.thr_low,
            skimmer_thr_high: self.skimmer.decoder_params.envelope.thr_high,
            skimmer_channel_timeout_secs: self.skimmer.channel_timeout_secs,
            skimmer_store_max_age_secs: self.skimmer.spot_store_max_age_secs,
            skimmer_max_decode_chars: self.skimmer.decoder_params.max_text_chars,
            min_spot_snr: self.min_spot_snr,
            spot_cq_only: self.spot_cq_only,
            spot_hide_heard_labels: self.spot_hide_heard_labels,
            spot_max_age_secs: self.spot_max_age_secs,
            spot_callsign_filter: self.spot_callsign_filter.clone(),
            spot_label_limit: self.spot_label_limit,
            scp_require: self.skimmer.require_scp,
            spot_sort: spot_sort_to_u8(self.spot_sort),
            continent_filter: self.continent_filter,
            show_continents: self.show_continents,
            show_console: self.show_console,
            filter_wide: self.filter_wide,
            show_history: self.show_history,
            show_left: self.show_left,
            show_right: self.show_right,
            show_af_scope: self.show_af_scope,
            show_smeter: self.show_smeter,
            recent_hosts: self.recent_hosts.clone(),
            last_center_mhz: self.center_khz / 1000.0,
            kiwi: self.form_kiwi.clone(),
            airspy: self.form_airspy.clone(),
            airspy_sample_rate: self.form_sample_rate,
            rtlsdr: self.form_rtlsdr.clone(),
            rtlsdr_sample_rate: self.form_sample_rate,
            qmx: self.form_qmx.clone(),
            settings_format: 1,
            iq_capture_dir: self.iq.capture_dir.display().to_string(),
            iq_playback_path: self.iq.playback_path.clone(),
        }
    }



    /// Debounced autosave: persist once settings have been stable for ~1s.
    fn autosave(&mut self) {
        let current = self.current_settings();
        if self.last_settings_snapshot.as_ref() != Some(&current) {
            self.last_settings_snapshot = Some(current);
            self.settings_dirty_at = Some(Instant::now());
        }
        if let Some(at) = self.settings_dirty_at {
            if at.elapsed() >= Duration::from_secs(1) {
                self.current_settings().save();
                self.settings_dirty_at = None;
            }
        }
    }



    fn invalidate_waterfall_history(&mut self) {
        self.rows.clear();
        self.force_texture_full = true;
        self.textures_dirty = true;
        self.last_viewport_key = None;
        self.last_storage_key = None;
        self.pending_row_appends = 0;
        self.pending_viewport_row_appends = 0;
        self.waterfall_storage_pixels.clear();
        self.waterfall_viewport_pixels.clear();
        self.storage_tex_width = 0;
        self.viewport_tex_width = 0;
        self.waterfall_viewport_texture = None;
    }

