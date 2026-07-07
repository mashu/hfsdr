use crate::app::WaterfallApp;
use crate::app::prelude::*;
use crate::waterfall_perf::WaterfallPerf;

impl WaterfallApp {

    pub(crate) fn apply_settings(&mut self, s: &AppSettings) {
        self.radio.cw.bfo_hz = s.bfo_hz;
        self.radio.cw.sideband = sideband_from_u8(s.cw_sideband);
        self.radio.sideband_auto = s.cw_sideband_auto;
        self.radio.cw.passband_hz = s.passband_hz;
        let mut channel_filter = channel_filter_from_u8(s.channel_filter);
        if s.economy_filter {
            channel_filter = ChannelFilterKind::Iir2Pole;
        }
        self.radio.cw.channel_filter = channel_filter;
        self.radio.cw.decim_filter = channel_filter_from_u8(s.decim_filter);
        self.radio.cw.iir_filter = iir_filter_from_u8(s.iir_filter);
        self.radio.cw.window = window_from_u8(s.window);
        self.radio.cw.kaiser_beta = s.kaiser_beta.clamp(MIN_KAISER_BETA, MAX_KAISER_BETA);
        self.radio.cw.passband_flatten = s.passband_flatten;
        self.radio.cw.deep_selectivity = s.deep_selectivity;
        self.radio.cw.passband_cutoff_frac = s
            .passband_cutoff_frac
            .clamp(hfsdr::MIN_PASSBAND_CUTOFF_FRAC, hfsdr::MAX_PASSBAND_CUTOFF_FRAC);
        self.radio.cw.dolph_sidelobe_db = s
            .dolph_sidelobe_db
            .clamp(hfsdr::MIN_DOLPH_SIDELOBE_DB, hfsdr::MAX_DOLPH_SIDELOBE_DB);
        self.radio.cw.detector_mode = detector_mode_from_u8(s.detector_mode);
        self.radio.cw.iq_apf.enabled = s.iq_apf_enabled;
        self.radio.cw.iq_apf.width_hz = s.iq_apf_width_hz;
        self.radio.cw.iq_apf.gain = s.iq_apf_gain;
        self.radio.cw.iq_wiener.enabled = s.iq_wiener_enabled;
        self.radio.cw.iq_wiener.level = s.iq_wiener_level;
        self.radio.cw.squelch.enabled = s.squelch_enabled;
        self.radio.cw.squelch.open_threshold = s.squelch_open_thr;
        self.radio.cw.squelch.close_threshold = s.squelch_close_thr;
        self.radio.cw.squelch.hang_ms = s.squelch_hang_ms;
        self.radio.cw.squelch.ramp_ms = s.squelch_ramp_ms;
        self.radio.cw.full_demod = s.full_demod;
        self.radio.cw.decimation = s.decimation;
        self.radio.cw.noise_blanker.enabled = s.nb_enabled;
        self.radio.cw.noise_blanker.threshold = s.nb_threshold;
        self.radio.cw.noise_blanker.width = s.nb_width as usize;
        self.radio.cw.auto_notch.enabled = s.an_enabled;
        self.radio.cw.auto_notch.guard_hz = s.an_guard_hz;
        self.radio.cw.auto_notch.rate = s.an_rate;
        self.radio.cw.apf.enabled = s.apf_enabled;
        self.radio.cw.apf.width_hz = s.apf_width_hz;
        self.radio.cw.apf.gain = s.apf_gain;
        self.radio.cw.noise_reduction.enabled = s.nr_enabled;
        self.radio.cw.noise_reduction.level = s.nr_level;
        self.radio.cw.agc.enabled = s.agc_enabled;
        self.radio.cw.agc.target = s.agc_target;
        self.radio.cw.agc.attack_ms = s.agc_attack_ms;
        self.radio.cw.agc.decay_ms = s.agc_decay_ms;
        self.radio.cw.agc.manual_gain = s.agc_manual_gain;
        self.radio.cw.agc.lookahead_ms = s.agc_lookahead_ms;
        self.radio.cw.agc_mode = agc_mode_from_u8(s.agc_mode);
        self.radio.cw.sidetone_envelope.enabled = s.st_envelope_enabled;
        self.radio.cw.sidetone_envelope.rise_ms = s.st_rise_ms;
        self.radio.cw.sidetone_envelope.fall_ms = s.st_fall_ms;
        self.radio.cw.sidetone_envelope.floor_gain = s.st_floor_gain;
        self.radio.cw.sidetone_envelope.shape =
            st_envelope_shape_from_u8(s.st_envelope_shape);
        for (slot, data) in self.radio.cw.notches.iter_mut().zip(s.notches.iter()) {
            slot.enabled = data.enabled;
            slot.offset_hz = ChannelOffsetHz::new(data.offset_hz);
            slot.width_hz = data.width_hz;
        }

        self.radio.rit_hz = s.rit_hz;
        self.radio.rit_on = s.rit_on || s.rit_hz.abs() > 0.5;
        self.radio.cw.filter_shift_hz = ChannelOffsetHz::new(s.filter_shift_hz);
        self.radio.passband_wide = s.filter_wide;
        if !self.radio.passband_wide && self.radio.cw.passband_hz > CW_PASSBAND_NARROW_MAX_HZ {
            self.radio.cw.passband_hz = CW_PASSBAND_NARROW_MAX_HZ;
        }
        self.radio.pitch_lock = s.pitch_lock;
        self.radio.lock_ham_bands = s.lock_ham_bands;
        self.radio.agc_rf_on = s.agc_rf_on;
        self.radio.last_agc_rf_on = s.agc_rf_on;
        self.radio.rf_gain_db = s.rf_gain_db.clamp(-20.0, 60.0);

        self.display.ref_db = s.ref_db;
        self.display.range_db = s.range_db;
        self.display.display_auto_track = s.display_auto_track;
        self.display.show_band_overview = s.show_band_overview;
        self.display.pan_step_hz = s.pan_step_hz.clamp(10.0, 50_000.0);
        self.display.pan_step_fast_hz = s.pan_step_fast_hz.clamp(50.0, 500_000.0);
        if self.display.display_auto_track {
            self.display.display_levels_initialized = false;
        } else {
            self.display.display_levels_initialized =
                display_levels_initialized_after_settings_load(self.display.display_auto_track);
        }
        self.display.smooth_alpha = s.smooth_alpha;
        self.display.waterfall_avg = normalize_waterfall_avg(s.waterfall_avg);
        self.display.spectrum_window = fft_window_from_u8(s.spectrum_window);
        self.display.spectrum_kaiser_beta =
            s.spectrum_kaiser_beta.clamp(MIN_KAISER_BETA, MAX_KAISER_BETA);
        self.display.target_fps = s.target_fps.clamp(10, 60);
        self.display.fft_size = s.fft_size.clamp(1024, 65_536);
        self.display.fft_auto = s.fft_auto;
        self.display.full_drain_spectrum = s.full_drain_spectrum;
        self.display.perf_trace = s.perf_trace;

        self.audio.audio_enabled = s.audio_enabled;
        self.audio.volume = s.volume;

        self.skimmer_ui.skimmer_enabled = s.skimmer_enabled;
        self.skimmer_ui.skimmer = skimmer_config_from_settings(s);
        self.skimmer_ui.min_spot_snr = s.min_spot_snr;
        self.skimmer_ui.spot_cq_only = s.spot_cq_only;
        self.skimmer_ui.spot_hide_heard_labels = s.spot_hide_heard_labels;
        self.skimmer_ui.spot_max_age_secs = s.spot_max_age_secs.max(0.0);
        self.skimmer_ui.spot_callsign_filter = s.spot_callsign_filter.clone();
        self.skimmer_ui.spot_label_limit = s.spot_label_limit.clamp(8, 80);
        self.skimmer_ui.spot_sort = spot_sort_from_u8(s.spot_sort);
        self.skimmer_ui.continent_filter = s.continent_filter;
        self.skimmer_ui.show_continents = s.show_continents;
        self.chrome.show_console = s.show_console;
        self.chrome.show_history = s.show_history;
        self.chrome.show_left = s.show_left;
        self.chrome.show_right = s.show_right;
        self.chrome.show_af_scope = s.show_af_scope;
        self.chrome.show_smeter = s.show_smeter;
        self.chrome.cw_simple_ui = s.cw_simple_ui;

        self.connection.form.recent_hosts = s.recent_hosts.clone();
        self.connection.form.kiwi = s.kiwi.clone();
        self.connection.form.kiwi.man_gain = s.kiwi_man_gain;
        self.radio.last_kiwi_man_gain = s.kiwi_man_gain;
        self.radio.last_kiwi_rf_attn_db = self.connection.form.kiwi.rf_attn_db;
        self.connection.form.airspy = s.airspy.clone();
        self.connection.form.rtlsdr = s.rtlsdr.clone();
        self.connection.form.qmx = s.qmx.clone();
        #[cfg(feature = "soapy")]
        {
            self.connection.form.soapy = s.soapy.clone();
        }
        if s.airspy_sample_rate != 0 {
            self.connection.form.sample_rate = s.airspy_sample_rate;
        } else if s.rtlsdr_sample_rate != 0 {
            self.connection.form.sample_rate = s.rtlsdr_sample_rate;
        }
        #[cfg(feature = "soapy")]
        if s.soapy_sample_rate != 0 {
            self.connection.form.sample_rate = s.soapy_sample_rate;
        }
        self.radio.center_khz = s.last_center_mhz * 1000.0;
        self.clamp_center_to_ham_bands();
        if self.radio.sideband_auto {
            self.sync_sideband_from_band();
        }
        self.radio.last_center_khz = self.radio.center_khz;
        self.chrome.iq.capture_dir = if s.iq_capture_dir.is_empty() {
            hfsdr::default_capture_dir()
        } else {
            std::path::PathBuf::from(&s.iq_capture_dir)
        };
        self.chrome.iq.playback_path = s.iq_playback_path.clone();
    }



    pub(crate) fn current_settings(&self) -> AppSettings {
        AppSettings {
            bfo_hz: self.radio.cw.bfo_hz,
            cw_sideband: sideband_to_u8(self.radio.cw.sideband),
            cw_sideband_auto: self.radio.sideband_auto,
            passband_hz: self.radio.cw.passband_hz,
            channel_filter: channel_filter_to_u8(self.radio.cw.channel_filter),
            decim_filter: channel_filter_to_u8(self.radio.cw.decim_filter),
            iir_filter: iir_filter_to_u8(self.radio.cw.iir_filter),
            window: window_to_u8(self.radio.cw.window),
            kaiser_beta: self.radio.cw.kaiser_beta,
            passband_flatten: self.radio.cw.passband_flatten,
            deep_selectivity: self.radio.cw.deep_selectivity,
            passband_cutoff_frac: self.radio.cw.passband_cutoff_frac,
            dolph_sidelobe_db: self.radio.cw.dolph_sidelobe_db,
            detector_mode: detector_mode_to_u8(self.radio.cw.detector_mode),
            iq_apf_enabled: self.radio.cw.iq_apf.enabled,
            iq_apf_width_hz: self.radio.cw.iq_apf.width_hz,
            iq_apf_gain: self.radio.cw.iq_apf.gain,
            iq_wiener_enabled: self.radio.cw.iq_wiener.enabled,
            iq_wiener_level: self.radio.cw.iq_wiener.level,
            squelch_enabled: self.radio.cw.squelch.enabled,
            squelch_open_thr: self.radio.cw.squelch.open_threshold,
            squelch_close_thr: self.radio.cw.squelch.close_threshold,
            squelch_hang_ms: self.radio.cw.squelch.hang_ms,
            squelch_ramp_ms: self.radio.cw.squelch.ramp_ms,
            economy_filter: false,
            full_demod: self.radio.cw.full_demod,
            decimation: self.radio.cw.decimation,
            nb_enabled: self.radio.cw.noise_blanker.enabled,
            nb_threshold: self.radio.cw.noise_blanker.threshold,
            nb_width: self.radio.cw.noise_blanker.width as u32,
            an_enabled: self.radio.cw.auto_notch.enabled,
            an_guard_hz: self.radio.cw.auto_notch.guard_hz,
            an_rate: self.radio.cw.auto_notch.rate,
            apf_enabled: self.radio.cw.apf.enabled,
            apf_width_hz: self.radio.cw.apf.width_hz,
            apf_gain: self.radio.cw.apf.gain,
            nr_enabled: self.radio.cw.noise_reduction.enabled,
            nr_level: self.radio.cw.noise_reduction.level,
            agc_enabled: self.radio.cw.agc.enabled,
            agc_target: self.radio.cw.agc.target,
            agc_attack_ms: self.radio.cw.agc.attack_ms,
            agc_decay_ms: self.radio.cw.agc.decay_ms,
            agc_manual_gain: self.radio.cw.agc.manual_gain,
            agc_lookahead_ms: self.radio.cw.agc.lookahead_ms,
            agc_mode: agc_mode_to_u8(self.radio.cw.agc_mode),
            st_envelope_enabled: self.radio.cw.sidetone_envelope.enabled,
            st_rise_ms: self.radio.cw.sidetone_envelope.rise_ms,
            st_fall_ms: self.radio.cw.sidetone_envelope.fall_ms,
            st_floor_gain: self.radio.cw.sidetone_envelope.floor_gain,
            st_envelope_shape: st_envelope_shape_to_u8(self.radio.cw.sidetone_envelope.shape),
            notches: self
                .radio.cw
                .notches
                .iter()
                .map(|n| NotchData {
                    enabled: n.enabled,
                    offset_hz: n.offset_hz.hz(),
                    width_hz: n.width_hz,
                })
                .collect(),
            rit_hz: self.radio.rit_hz,
            rit_on: self.radio.rit_on,
            filter_shift_hz: self.radio.cw.filter_shift_hz.hz(),
            pitch_lock: self.radio.pitch_lock,
            lock_ham_bands: self.radio.lock_ham_bands,
            agc_rf_on: self.radio.agc_rf_on,
            rf_gain_db: self.radio.rf_gain_db,
            kiwi_man_gain: self.connection.form.kiwi.man_gain,
            ref_db: self.display.ref_db,
            range_db: self.display.range_db,
            display_auto_track: self.display.display_auto_track,
            show_band_overview: self.display.show_band_overview,
            pan_step_hz: self.display.pan_step_hz,
            pan_step_fast_hz: self.display.pan_step_fast_hz,
            smooth_alpha: self.display.smooth_alpha,
            waterfall_avg: self.display.waterfall_avg,
            spectrum_window: fft_window_to_u8(self.display.spectrum_window),
            spectrum_kaiser_beta: self.display.spectrum_kaiser_beta,
            target_fps: self.display.target_fps,
            fft_size: self.display.fft_size,
            fft_auto: self.display.fft_auto,
            full_drain_spectrum: self.display.full_drain_spectrum,
            perf_trace: self.display.perf_trace,
            audio_enabled: self.audio.audio_enabled,
            volume: self.audio.volume,
            skimmer_enabled: self.skimmer_ui.skimmer_enabled,
            skimmer_min_snr_db: self.skimmer_ui.skimmer.min_snr_db,
            skimmer_min_decode_snr_db: self.skimmer_ui.skimmer.min_decode_snr_db,
            skimmer_decode_gate_ms: self.skimmer_ui.skimmer.decode_gate_ms,
            skimmer_max_channels: self.skimmer_ui.skimmer.max_channels,
            skimmer_bucket_hz: self.skimmer_ui.skimmer.bucket_hz,
            skimmer_min_separation_bins: self.skimmer_ui.skimmer.min_separation_bins,
            skimmer_decoder: skimmer_decoder_to_u8(self.skimmer_ui.skimmer.decoder),
            skimmer_beam_width: self.skimmer_ui.skimmer.decoder_params.beam_width,
            skimmer_lpf_cutoff_hz: self.skimmer_ui.skimmer.lpf_cutoff_hz,
            skimmer_initial_wpm: self.skimmer_ui.skimmer.decoder_params.initial_wpm,
            skimmer_thr_low: self.skimmer_ui.skimmer.decoder_params.envelope.thr_low,
            skimmer_thr_high: self.skimmer_ui.skimmer.decoder_params.envelope.thr_high,
            skimmer_channel_timeout_secs: self.skimmer_ui.skimmer.channel_timeout_secs,
            skimmer_store_max_age_secs: self.skimmer_ui.skimmer.spot_store_max_age_secs,
            skimmer_max_decode_chars: self.skimmer_ui.skimmer.decoder_params.max_text_chars,
            min_spot_snr: self.skimmer_ui.min_spot_snr,
            spot_cq_only: self.skimmer_ui.spot_cq_only,
            spot_hide_heard_labels: self.skimmer_ui.spot_hide_heard_labels,
            spot_max_age_secs: self.skimmer_ui.spot_max_age_secs,
            spot_callsign_filter: self.skimmer_ui.spot_callsign_filter.clone(),
            spot_label_limit: self.skimmer_ui.spot_label_limit,
            scp_require: self.skimmer_ui.skimmer.require_scp,
            spot_sort: spot_sort_to_u8(self.skimmer_ui.spot_sort),
            continent_filter: self.skimmer_ui.continent_filter,
            show_continents: self.skimmer_ui.show_continents,
            show_console: self.chrome.show_console,
            filter_wide: self.radio.passband_wide,
            show_history: self.chrome.show_history,
            show_left: self.chrome.show_left,
            show_right: self.chrome.show_right,
            show_af_scope: self.chrome.show_af_scope,
            show_smeter: self.chrome.show_smeter,
            cw_simple_ui: self.chrome.cw_simple_ui,
            recent_hosts: self.connection.form.recent_hosts.clone(),
            last_center_mhz: self.radio.center_khz / 1000.0,
            kiwi: self.connection.form.kiwi.clone(),
            airspy: self.connection.form.airspy.clone(),
            airspy_sample_rate: self.connection.form.sample_rate,
            rtlsdr: self.connection.form.rtlsdr.clone(),
            rtlsdr_sample_rate: self.connection.form.sample_rate,
            qmx: self.connection.form.qmx.clone(),
            #[cfg(feature = "soapy")]
            soapy: self.connection.form.soapy.clone(),
            #[cfg(feature = "soapy")]
            soapy_sample_rate: self.connection.form.sample_rate,
            settings_format: 1,
            iq_capture_dir: self.chrome.iq.capture_dir.display().to_string(),
            iq_playback_path: self.chrome.iq.playback_path.clone(),
        }
    }



    /// Debounced autosave: persist once settings have been stable for ~1s.
    pub(crate) fn autosave(&mut self) {
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



    pub(crate) fn invalidate_waterfall_history(&mut self) {
        self.plot.rows.clear();
        self.plot.waterfall.force_texture_full = true;
        self.plot.waterfall.textures_dirty = true;
        self.plot.waterfall.last_viewport_key = None;
        self.plot.waterfall.last_storage_key = None;
        self.plot.waterfall.pending_row_appends = 0;
        self.plot.waterfall.pending_viewport_row_appends = 0;
        self.plot.waterfall.storage_pixels.clear();
        self.plot.waterfall.viewport_pixels.clear();
        self.plot.waterfall.storage_tex_width = 0;
        self.plot.waterfall.viewport_tex_width = 0;
        self.plot.waterfall.viewport_row_head = 0;
        self.plot.waterfall.viewport_texture = None;
        self.plot.waterfall.trace_refresh = false;
        self.plot.waterfall.perf = WaterfallPerf::default();
    }


}
