//! Waterfall application state and rendering.
//!
//! The UI thread owns no DSP: it pushes settings to the [`crate::engine`] worker,
//! drains spectrum rows / status / spots it publishes, renders, and repaints
//! lazily. Connection lifecycle (connect, slow/unstable warnings, auto-reconnect)
//! is driven by the engine and surfaced here.

use std::collections::HashSet;
use std::collections::VecDeque;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use eframe::egui;
use egui::Color32;
use egui_extras::{Column, TableBuilder};
use hfsdr::{
    decimation_factor, extract_view_window, spectrum_view_mapping, strongest_offset_hz, Continent,
    ContinentResolver, CwChannelSettings, RowFold, SlowWaterfall, SpectrumViewMapping, Spot,
    SpotKind, SpotSort, SkimmerConfig, SkimmerDecoderKind, channel_group_delay_ms, WindowKind,
    MAX_NOTCHES,
};

use crate::audio::AudioOutput;
use crate::colormap::db_to_colour;
use crate::controls::{scroll_drag_f64, scroll_slider_f32, scroll_slider_log_f32};
use crate::display_levels::{estimate_levels, estimate_levels_from_rows};
use crate::engine::{
    ConnState, EngineCommand, EngineHandle, EngineParams, EngineStats, FFT_SIZE, WATERFALL_ROWS,
};
use crate::interaction::{PlotAction, PlotInteraction, PlotViewState};
use crate::interaction::{CW_PASSBAND_MAX_HZ, CW_PASSBAND_MIN_HZ, CW_PASSBAND_NARROW_MAX_HZ};
use crate::kiwi_directory::{GeoLocation, KiwiReceiver};
use crate::log;
use crate::settings::{AppSettings, NotchData};
use crate::source::{ConnectRequest, SourceKind};
use crate::spot_filter::{
    build_spot_labels, continent_index, filter_spots, SpotFilterConfig, SpotLabelConfig,
};
use crate::theme::{
    apply, clickable_badge, collapsible_section, section_card, section_heading, section_hint,
    stat_row, toggle, MUTED, OK, WARN,
};
use crate::widgets::{display_trace, SpectrumWidget, SpotLabel, WaterfallWidget};

const SMOOTH_ALPHA: f32 = 0.09;

/// CW segment / calling-frequency anchors across the HF bands (Hz).
const BAND_PRESETS: [(&str, f64); 11] = [
    ("160m", 1_810_000.0),
    ("80m", 3_510_000.0),
    ("60m", 5_354_000.0),
    ("40m", 7_010_000.0),
    ("30m", 10_110_000.0),
    ("20m", 14_010_000.0),
    ("17m", 18_080_000.0),
    ("15m", 21_010_000.0),
    ("12m", 24_900_000.0),
    ("10m", 28_010_000.0),
    ("6m", 50_090_000.0),
];

const BFO_PRESETS: [(&str, f32); 4] = [("500", 500.0), ("600", 600.0), ("700", 700.0), ("800", 800.0)];
const FILTER_PRESETS: [(&str, f32); 6] = [
    ("50", 50.0),
    ("100", 100.0),
    ("250", 250.0),
    ("500", 500.0),
    ("1k", 1_000.0),
    ("2k", 2_000.0),
];

pub struct WaterfallApp {
    engine: EngineHandle,
    conn_state: ConnState,
    stats: EngineStats,
    last_error: Option<String>,
    pending_connect: Option<ConnectRequest>,

    // Connection form.
    form_kind: SourceKind,
    form_host: String,
    form_port: u16,
    form_center_mhz: f64,

    sample_rate: f32,
    center_khz: f64,
    last_center_khz: f64,
    is_kiwi: bool,

    /// The toggleable CW listen-chain configuration (owned by the UI).
    cw: CwChannelSettings,
    rit_hz: f32,
    pitch_lock: bool,
    agc_rf_on: bool,
    last_agc_rf_on: bool,
    last_snr_db: f32,

    rows: VecDeque<Vec<f32>>,
    latest: Vec<f32>,
    smoothed_trace: Vec<f32>,
    texture: Option<egui::TextureHandle>,
    textures_dirty: bool,
    last_tex_span: f32,
    last_tex_pan: f64,
    last_tex_row_rate: f32,
    last_spectrum_pan: f64,
    last_spectrum_span: f32,
    last_spectrum_zoomed: bool,

    ref_db: f32,
    range_db: f32,
    display_levels_initialized: bool,
    display_auto_track: bool,
    smooth_alpha: f32,
    waterfall_rows: usize,
    target_fps: u32,
    fft_size: usize,
    fft_auto: bool,

    audio_devices: Vec<String>,
    selected_audio_device: usize,
    last_audio_device: usize,
    audio_enabled: bool,
    volume: f32,

    skimmer_enabled: bool,
    skimmer: SkimmerConfig,
    skimmer_channels: usize,
    skimmer_spots: Vec<Spot>,
    spot_sort: SpotSort,
    continent_filter: bool,
    show_continents: [bool; 7],
    min_spot_snr: f32,
    spot_cq_only: bool,
    spot_hide_heard_labels: bool,
    spot_max_age_secs: f32,
    spot_callsign_filter: String,
    spot_label_limit: usize,
    scp_notice: Option<String>,
    scp_download_rx: Option<Receiver<Result<std::path::PathBuf, String>>>,
    scp_reload_pending: bool,
    scp_reload_deadline: Option<Instant>,
    last_scp_loaded: bool,
    filter_wide: bool,
    show_console: bool,
    frame_visible_spots: Vec<Spot>,
    resolver: ContinentResolver,
    annotated: HashSet<String>,
    slow: SlowWaterfall,
    show_history: bool,
    show_left: bool,
    show_right: bool,

    recent_hosts: Vec<ConnectRequest>,
    kiwi_geo: Option<GeoLocation>,
    kiwi_nearby: Vec<KiwiReceiver>,
    kiwi_directory_rx: Option<Receiver<Result<(Option<GeoLocation>, Vec<KiwiReceiver>), String>>>,
    kiwi_directory_error: Option<String>,
    show_connection_drawer: bool,

    last_settings_json: String,
    settings_dirty_at: Option<std::time::Instant>,

    spectrum_widget: SpectrumWidget,
    waterfall_widget: WaterfallWidget,
    plot_view: PlotViewState,
    plot_interaction: PlotInteraction,
    hover_offset_hz: Option<f64>,
    tune_preview_offset_hz: Option<f64>,
    themed: bool,
}

impl WaterfallApp {
    pub fn new(autoconnect: Option<ConnectRequest>) -> Self {
        let saved = AppSettings::load();
        let audio_devices = AudioOutput::list_output_devices();

        let mut app = Self {
            engine: EngineHandle::spawn(),
            conn_state: ConnState::Disconnected,
            stats: EngineStats::default(),
            last_error: None,
            pending_connect: None,
            form_kind: SourceKind::Kiwi,
            form_host: String::new(),
            form_port: 8073,
            form_center_mhz: 7.03,
            sample_rate: 12_000.0,
            center_khz: 7_030.0,
            last_center_khz: 7_030.0,
            is_kiwi: false,
            cw: CwChannelSettings::default(),
            rit_hz: 0.0,
            pitch_lock: false,
            agc_rf_on: true,
            last_agc_rf_on: true,
            last_snr_db: 0.0,
            rows: VecDeque::with_capacity(WATERFALL_ROWS),
            latest: vec![-120.0; FFT_SIZE],
            smoothed_trace: Vec::new(),
            texture: None,
            textures_dirty: false,
            last_tex_span: 0.0,
            last_tex_pan: 0.0,
            last_tex_row_rate: 0.0,
            last_spectrum_pan: 0.0,
            last_spectrum_span: 0.0,
            last_spectrum_zoomed: false,
            ref_db: -65.0,
            range_db: crate::display_levels::DEFAULT_RANGE_DB,
            display_levels_initialized: false,
            display_auto_track: false,
            smooth_alpha: SMOOTH_ALPHA,
            waterfall_rows: 0,
            target_fps: 30,
            fft_size: FFT_SIZE,
            fft_auto: true,
            audio_devices,
            selected_audio_device: 0,
            last_audio_device: 0,
            audio_enabled: true,
            volume: 1.0,
            skimmer_enabled: true,
            skimmer: SkimmerConfig::default(),
            skimmer_channels: 0,
            skimmer_spots: Vec::new(),
            spot_sort: SpotSort::SnrDesc,
            continent_filter: false,
            show_continents: [true; 7],
            min_spot_snr: 12.0,
            spot_cq_only: false,
            spot_hide_heard_labels: true,
            spot_max_age_secs: 90.0,
            spot_callsign_filter: String::new(),
            spot_label_limit: 40,
            scp_notice: None,
            scp_download_rx: None,
            scp_reload_pending: false,
            scp_reload_deadline: None,
            last_scp_loaded: false,
            filter_wide: false,
            show_console: false,
            frame_visible_spots: Vec::new(),
            resolver: ContinentResolver::new(),
            annotated: HashSet::new(),
            slow: SlowWaterfall::new(2.0, 600.0, RowFold::Peak),
            show_history: false,
            show_left: true,
            show_right: true,
            recent_hosts: Vec::new(),
            kiwi_geo: None,
            kiwi_nearby: Vec::new(),
            kiwi_directory_rx: None,
            kiwi_directory_error: None,
            show_connection_drawer: false,
            last_settings_json: String::new(),
            settings_dirty_at: None,
            spectrum_widget: SpectrumWidget::new(),
            waterfall_widget: WaterfallWidget::new(),
            plot_view: PlotViewState::new(),
            plot_interaction: PlotInteraction::new(),
            hover_offset_hz: None,
            tune_preview_offset_hz: None,
            themed: false,
        };

        app.apply_settings(&saved);

        // Seed the connect form from the most-recent host.
        if let Some(r) = app.recent_hosts.first().cloned() {
            app.form_kind = r.kind;
            app.form_host = r.host;
            app.form_port = r.port;
            app.form_center_mhz = r.center_hz / 1e6;
        }

        // CLI args take precedence and trigger an auto-connect on first frame.
        if let Some(req) = autoconnect {
            app.form_kind = req.kind;
            app.form_host = req.host.clone();
            app.form_port = req.port;
            app.form_center_mhz = req.center_hz / 1e6;
            app.center_khz = req.center_hz / 1000.0;
            app.last_center_khz = app.center_khz;
            app.pending_connect = Some(req);
            app.show_connection_drawer = false;
        }

        app.last_settings_json =
            serde_json::to_string(&app.current_settings()).unwrap_or_default();
        if let Some((geo, receivers)) = crate::kiwi_directory::load_cached_receivers() {
            app.kiwi_geo = geo;
            app.kiwi_nearby = receivers;
        }
        app.start_kiwi_directory_fetch(false);
        app
    }

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

    fn apply_settings(&mut self, s: &AppSettings) {
        self.cw.bfo_hz = s.bfo_hz;
        self.cw.passband_hz = s.passband_hz;
        self.cw.window = window_from_u8(s.window);
        self.cw.decimation = s.decimation;
        self.cw.squelch = s.squelch;
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
        for (slot, data) in self.cw.notches.iter_mut().zip(s.notches.iter()) {
            slot.enabled = data.enabled;
            slot.offset_hz = data.offset_hz;
            slot.width_hz = data.width_hz;
        }

        self.rit_hz = s.rit_hz;
        self.pitch_lock = s.pitch_lock;
        self.agc_rf_on = s.agc_rf_on;
        self.last_agc_rf_on = s.agc_rf_on;

        self.ref_db = s.ref_db;
        self.range_db = s.range_db;
        self.display_auto_track = s.display_auto_track;
        if !self.display_auto_track {
            self.display_levels_initialized = false;
        }
        self.smooth_alpha = s.smooth_alpha;
        self.target_fps = s.target_fps.clamp(10, 60);
        self.fft_size = s.fft_size.clamp(1024, 65_536);
        self.fft_auto = s.fft_auto;

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

        self.recent_hosts = s.recent_hosts.clone();
        self.center_khz = s.last_center_mhz * 1000.0;
        self.last_center_khz = self.center_khz;
    }

    fn current_settings(&self) -> AppSettings {
        AppSettings {
            bfo_hz: self.cw.bfo_hz,
            passband_hz: self.cw.passband_hz,
            window: window_to_u8(self.cw.window),
            decimation: self.cw.decimation,
            squelch: self.cw.squelch,
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
            notches: self
                .cw
                .notches
                .iter()
                .map(|n| NotchData {
                    enabled: n.enabled,
                    offset_hz: n.offset_hz,
                    width_hz: n.width_hz,
                })
                .collect(),
            rit_hz: self.rit_hz,
            pitch_lock: self.pitch_lock,
            agc_rf_on: self.agc_rf_on,
            ref_db: self.ref_db,
            range_db: self.range_db,
            display_auto_track: self.display_auto_track,
            smooth_alpha: self.smooth_alpha,
            target_fps: self.target_fps,
            fft_size: self.fft_size,
            fft_auto: self.fft_auto,
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
            recent_hosts: self.recent_hosts.clone(),
            last_center_mhz: self.center_khz / 1000.0,
        }
    }

    /// Debounced autosave: persist once settings have been stable for ~1s.
    fn autosave(&mut self) {
        let json = serde_json::to_string(&self.current_settings()).unwrap_or_default();
        if json != self.last_settings_json {
            self.last_settings_json = json;
            self.settings_dirty_at = Some(Instant::now());
        }
        if let Some(at) = self.settings_dirty_at {
            if at.elapsed() >= Duration::from_secs(1) {
                self.current_settings().save();
                self.settings_dirty_at = None;
            }
        }
    }

    /// Push UI settings to the engine and pull its published rows/status/spots.
    fn pump_engine(&mut self) {
        self.cw.listen_offset_hz = self.listen_offset_hz() as f32;
        self.plot_view.clamp_pan(self.sample_rate);
        let view = self.spectrum_view();
        self.skimmer = self.skimmer.clone().clamped();
        self.engine.set_params(EngineParams {
            cw: self.cw.clone(),
            audio_enabled: self.audio_enabled,
            volume: self.volume,
            skimmer_enabled: self.skimmer_enabled,
            skimmer: self.skimmer.clone(),
            fft_size: self.fft_size,
            fft_auto: self.fft_auto,
            view_span_hz: view.view_span_hz,
            view_pan_offset_hz: self.plot_view.pan_offset_hz,
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
        let latest = poll.latest;
        let new_rows = poll.rows;
        if latest.len() != self.latest.len() {
            // FFT size changed under us: adopt the new width and reset buffers.
            self.latest = latest;
            self.rows.clear();
            self.textures_dirty = true;
        } else {
            self.latest.copy_from_slice(&latest);
        }

        self.sample_rate = self.stats.sample_rate;
        self.is_kiwi = self.stats.is_kiwi;
        self.last_snr_db = self.stats.snr_db;
        self.skimmer_channels = self.stats.skimmer_channels;
        if self.fft_auto {
            self.fft_size = self.stats.spectrum_fft.max(1024);
        }

        let view_span = self.plot_view.view_span_hz(self.sample_rate);
        let view_pan = self.plot_view.pan_offset_hz;
        let zoomed = self.stats.spectrum_zoomed;
        if zoomed
            && ((view_pan - self.last_spectrum_pan).abs() > 0.5
                || (view_span - self.last_spectrum_span).abs() > 1.0
                || zoomed != self.last_spectrum_zoomed)
        {
            // Zoomed rows are mix-down specific — stale pan poisons the waterfall.
            self.rows.clear();
            self.textures_dirty = true;
        }
        self.last_spectrum_pan = view_pan;
        self.last_spectrum_span = view_span;
        self.last_spectrum_zoomed = zoomed;

        if !new_rows.is_empty() {
            for row in new_rows {
                let mut stored = if self.rows.len() >= WATERFALL_ROWS {
                    self.rows.pop_back().unwrap_or_else(|| vec![0.0; row.len()])
                } else {
                    vec![0.0; row.len()]
                };
                if stored.len() == row.len() {
                    stored.copy_from_slice(&row);
                    self.rows.push_front(stored);
                }
            }
            self.waterfall_rows = self.rows.len();
            self.textures_dirty = true;
            self.update_display_levels();
        }

        self.apply_pitch_lock();
        if self.skimmer_enabled {
            self.annotate_new_spots(self.center_khz * 1000.0);
        }
    }

    fn apply_plot_actions(&mut self, actions: Vec<PlotAction>) {
        for action in actions {
            match action {
                PlotAction::TuneDeltaHz(delta) | PlotAction::CenterOnOffsetHz(delta) => {
                    self.center_khz += delta / 1000.0;
                    self.plot_view.pan_offset_hz = 0.0;
                    self.tune_preview_offset_hz = None;
                }
                PlotAction::SetTunePreviewOffsetHz(offset) => {
                    self.tune_preview_offset_hz = Some(offset);
                }
                PlotAction::CommitTunePreview => {
                    if let Some(offset) = self.tune_preview_offset_hz {
                        self.center_khz += offset / 1000.0;
                        self.plot_view.pan_offset_hz = 0.0;
                    }
                    self.tune_preview_offset_hz = None;
                }
                PlotAction::ClearTunePreview => {
                    self.tune_preview_offset_hz = None;
                }
                PlotAction::PanViewDeltaHz(delta) => {
                    self.plot_view.pan_offset_hz += delta;
                    self.plot_view.clamp_pan(self.sample_rate);
                }
                PlotAction::ZoomView(factor) => {
                    self.plot_view.zoom_by(factor, self.sample_rate);
                }
                PlotAction::SetPassbandHz(bw) => {
                    self.cw.passband_hz =
                        bw.clamp(CW_PASSBAND_MIN_HZ, self.passband_max_hz());
                }
            }
        }
    }

    fn annotate_new_spots(&mut self, center_hz: f64) {
        for spot in &self.skimmer_spots {
            let Some(call) = &spot.callsign else { continue };
            let key = format!("{call}@{:.0}", spot.frequency_hz);
            if self.annotated.insert(key) {
                let offset = (spot.frequency_hz - center_hz) as f32;
                let label = match spot.kind {
                    SpotKind::CallingCq => format!("CQ {call}"),
                    SpotKind::Answering => format!("→ {call}"),
                    SpotKind::Heard => call.clone(),
                };
                self.slow.annotate(offset, label, spot.snr_db);
            }
        }
        if self.annotated.len() > 512 {
            self.annotated.clear();
        }
    }

    fn update_display_levels(&mut self) {
        if self.display_levels_initialized && !self.display_auto_track {
            return;
        }
        let target = self.estimate_display_levels();
        let Some(target) = target else {
            return;
        };
        let (ref_db, range_db) = if self.display_auto_track && self.display_levels_initialized {
            crate::display_levels::smooth_levels(
                (self.ref_db, self.range_db),
                target,
                0.06,
            )
        } else {
            target
        };
        self.ref_db = ref_db;
        self.range_db = range_db;
        self.textures_dirty = true;
        self.display_levels_initialized = true;
    }

    fn estimate_display_levels(&self) -> Option<(f32, f32)> {
        const ROWS_FOR_ESTIMATE: usize = 24;
        if self.rows.len() >= 8 {
            let n = self.rows.len().min(ROWS_FOR_ESTIMATE);
            let refs: Vec<&[f32]> = self.rows.iter().take(n).map(Vec::as_slice).collect();
            estimate_levels_from_rows(&refs).or_else(|| estimate_levels(&self.latest))
        } else {
            estimate_levels(&self.latest)
        }
    }

    fn spectrum_view(&self) -> SpectrumViewMapping {
        let span = self.plot_view.view_span_hz(self.sample_rate);
        spectrum_view_mapping(
            self.sample_rate,
            self.stats.spectrum_rate,
            self.stats.spectrum_zoomed,
            span,
            self.plot_view.pan_offset_hz,
        )
    }

    /// Snap tuning so the strongest signal near the cursor lands at the BFO pitch.
    fn zero_beat(&mut self) {
        let listen = self.listen_offset_hz() as f32;
        let pan = self.plot_view.pan_offset_hz as f32;
        let view = self.spectrum_view();
        let search = if self.stats.spectrum_zoomed {
            listen - pan
        } else {
            listen
        };
        if let Some(peak) = strongest_offset_hz(&self.latest, view.row_rate_hz, search, 400.0) {
            let from_center = if self.stats.spectrum_zoomed {
                peak + pan
            } else {
                peak
            };
            self.center_khz += (from_center - listen) as f64 / 1000.0;
            self.rit_hz = 0.0;
            self.tune_preview_offset_hz = None;
        }
    }

    /// Continuously steer RIT so a drifting signal keeps a constant audio pitch.
    fn apply_pitch_lock(&mut self) {
        if !self.pitch_lock {
            return;
        }
        let listen = self.listen_offset_hz() as f32;
        let pan = self.plot_view.pan_offset_hz as f32;
        let view = self.spectrum_view();
        let search = if self.stats.spectrum_zoomed {
            listen - pan
        } else {
            listen
        };
        if let Some(peak) = strongest_offset_hz(&self.latest, view.row_rate_hz, search, 250.0) {
            let from_center = if self.stats.spectrum_zoomed {
                peak + pan
            } else {
                peak
            };
            let preview = self.tune_preview_offset_hz.unwrap_or(0.0) as f32;
            let target = (from_center - preview).clamp(-800.0, 800.0);
            self.rit_hz = 0.85 * self.rit_hz + 0.15 * target;
        }
    }

    fn apply_audio_device(&mut self) {
        if self.selected_audio_device == self.last_audio_device {
            return;
        }
        let name = self.audio_devices.get(self.selected_audio_device).cloned();
        self.engine.send(EngineCommand::SetAudioDevice(name));
        self.last_audio_device = self.selected_audio_device;
    }

    fn listen_offset_hz(&self) -> f64 {
        self.rit_hz as f64 + self.tune_preview_offset_hz.unwrap_or(0.0)
    }

    fn enabled_notches(&self) -> Vec<(f32, f32)> {
        self.cw
            .notches
            .iter()
            .filter(|n| n.enabled)
            .map(|n| (n.offset_hz, n.width_hz))
            .collect()
    }

    fn spot_filter_config(&self) -> SpotFilterConfig {
        SpotFilterConfig {
            min_snr_db: self.min_spot_snr,
            cq_only: self.spot_cq_only,
            max_age_secs: self.spot_max_age_secs,
            callsign_prefix: self.spot_callsign_filter.clone(),
            continent_filter: self.continent_filter,
            show_continents: self.show_continents,
            sort: self.spot_sort,
        }
    }

    fn visible_spots(&self) -> Vec<Spot> {
        filter_spots(
            &self.skimmer_spots,
            &self.spot_filter_config(),
            &self.resolver,
        )
    }

    fn spot_labels(&self, center_hz: f64) -> Vec<SpotLabel> {
        build_spot_labels(
            &self.frame_visible_spots,
            center_hz,
            &SpotLabelConfig {
                hide_heard: self.spot_hide_heard_labels,
                bucket_hz: self.skimmer.bucket_hz,
                label_limit: self.spot_label_limit,
            },
        )
    }

    fn clear_spots(&mut self) {
        self.engine.send(EngineCommand::ClearSkimmerSpots);
        self.skimmer_spots.clear();
        self.frame_visible_spots.clear();
        self.annotated.clear();
        log::info("spots cleared");
    }

    fn poll_scp_download(&mut self) {
        let Some(rx) = self.scp_download_rx.as_ref() else {
            return;
        };
        let Ok(result) = rx.try_recv() else {
            return;
        };
        self.scp_download_rx = None;
        match result {
            Ok(path) => {
                log::info(format!("MASTER.SCP saved to {}", path.display()));
                self.engine.send(EngineCommand::ReloadScpFrom(path.clone()));
                self.scp_reload_pending = true;
                self.scp_reload_deadline = Some(Instant::now() + Duration::from_secs(8));
                self.scp_notice = Some(format!("Downloaded — loading {}", path.display()));
            }
            Err(e) => {
                log::error(format!("MASTER.SCP download failed: {e}"));
                self.scp_notice = Some(format!("Download failed: {e}"));
            }
        }
    }

    fn scp_section(&mut self, ui: &mut egui::Ui) {
        let scp = &self.stats.scp;
        let downloading = self.scp_download_rx.is_some();
        collapsible_section(ui, "scp", "MASTER.SCP", false, |ui| {
            if scp.loaded {
                let ver = scp.version.as_deref().unwrap_or("unknown version");
                stat_row(ui, "Database", format!("{} calls ({ver})", scp.calls));
                if let Some(path) = &scp.path {
                    stat_row(ui, "Path", path.clone());
                }
            } else {
                ui.colored_label(
                    WARN,
                    "Not loaded — using heuristic callsign check (more false positives)",
                );
                section_hint(ui, "Install N1MM+ MASTER.SCP or click Download below.");
            }
            if let Some(msg) = &self.scp_notice {
                ui.colored_label(OK, msg);
            }
            ui.horizontal(|ui| {
                ui.add_enabled_ui(!downloading, |ui| {
                    if ui.button("Download").clicked() {
                        let (tx, rx) = std::sync::mpsc::channel();
                        self.scp_download_rx = Some(rx);
                        self.scp_notice = Some("Downloading MASTER.SCP…".into());
                        std::thread::spawn(move || {
                            let _ = tx.send(crate::scp_fetch::download_master_scp());
                        });
                    }
                });
                if downloading {
                    ui.spinner();
                }
                if ui.button("Reload").clicked() {
                    self.engine.send(EngineCommand::ReloadScp);
                    self.scp_reload_pending = true;
                    self.scp_reload_deadline = Some(Instant::now() + Duration::from_secs(8));
                    self.scp_notice = Some("Reloading MASTER.SCP…".into());
                    log::info("MASTER.SCP reload requested");
                }
            });
        });
    }

    fn tune_to_hz(&mut self, frequency_hz: f64) {
        self.center_khz = frequency_hz / 1000.0;
        self.plot_view.pan_offset_hz = 0.0;
        self.tune_preview_offset_hz = None;
        self.rit_hz = 0.0;
    }

    fn apply_radio_settings(&mut self) {
        if (self.center_khz - self.last_center_khz).abs() > f64::EPSILON {
            self.engine.send(EngineCommand::Tune(self.center_khz * 1000.0));
            self.last_center_khz = self.center_khz;
        }
        if self.is_kiwi && self.agc_rf_on != self.last_agc_rf_on {
            self.engine.send(EngineCommand::SetRfAgc(self.agc_rf_on));
            self.last_agc_rf_on = self.agc_rf_on;
        }
        self.apply_audio_device();
    }

    fn connect_now(&mut self) {
        let req = ConnectRequest {
            kind: self.form_kind,
            host: self.form_host.trim().to_string(),
            port: self.form_port,
            center_hz: self.form_center_mhz * 1e6,
            sample_rate: 0,
        };
        self.center_khz = req.center_hz / 1000.0;
        self.last_center_khz = self.center_khz;
        self.remember_host(&req);
        log::info(format!("connecting to {}", req.label()));
        self.engine.send(EngineCommand::Connect(req));
    }

    fn remember_host(&mut self, req: &ConnectRequest) {
        self.recent_hosts.retain(|r| r != req);
        self.recent_hosts.insert(0, req.clone());
        self.recent_hosts.truncate(8);
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let (zero, lock, notch, blank, nr, agc, narrow, widen, rit_dn, rit_up, full, mute, console, f11) =
            ctx.input(|i| {
                use egui::Key;
                (
                    i.key_pressed(Key::Z),
                    i.key_pressed(Key::L),
                    i.key_pressed(Key::N),
                    i.key_pressed(Key::B),
                    i.key_pressed(Key::R),
                    i.key_pressed(Key::A),
                    i.key_pressed(Key::OpenBracket),
                    i.key_pressed(Key::CloseBracket),
                    i.key_pressed(Key::Comma),
                    i.key_pressed(Key::Period),
                    i.key_pressed(Key::F),
                    i.key_pressed(Key::Space),
                    i.key_pressed(Key::Backtick),
                    i.key_pressed(Key::F11),
                )
            });

        if zero {
            self.zero_beat();
        }
        if lock {
            self.pitch_lock = !self.pitch_lock;
        }
        if notch {
            self.cw.auto_notch.enabled = !self.cw.auto_notch.enabled;
        }
        if blank {
            self.cw.noise_blanker.enabled = !self.cw.noise_blanker.enabled;
        }
        if nr {
            self.cw.noise_reduction.enabled = !self.cw.noise_reduction.enabled;
        }
        if agc {
            self.cw.agc.enabled = !self.cw.agc.enabled;
        }
        if narrow {
            self.cw.passband_hz =
                (self.cw.passband_hz - 25.0).clamp(CW_PASSBAND_MIN_HZ, self.passband_max_hz());
        }
        if widen {
            self.cw.passband_hz =
                (self.cw.passband_hz + 25.0).clamp(CW_PASSBAND_MIN_HZ, self.passband_max_hz());
        }
        if rit_dn {
            self.rit_hz = (self.rit_hz - 10.0).clamp(-800.0, 800.0);
        }
        if rit_up {
            self.rit_hz = (self.rit_hz + 10.0).clamp(-800.0, 800.0);
        }
        if full {
            self.plot_view.zoom = 1.0;
            self.plot_view.pan_offset_hz = 0.0;
        }
        if mute {
            self.audio_enabled = !self.audio_enabled;
        }
        if console {
            self.show_console = !self.show_console;
        }
        if f11 {
            let on = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!on));
        }
    }

    fn console_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Log").strong());
            if ui.button("Clear").clicked() {
                log::clear();
            }
        });
        egui::ScrollArea::vertical()
            .max_height(140.0)
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for line in log::entries() {
                    ui.label(
                        egui::RichText::new(line)
                            .monospace()
                            .size(11.0)
                            .color(MUTED),
                    );
                }
            });
    }

    fn update_texture(&mut self, ctx: &egui::Context) {
        let view = self.spectrum_view();
        let row = extract_view_window(
            &self.latest,
            view.row_rate_hz,
            view.view_span_hz,
            view.pan_offset_hz,
        );
        let w = row.len().max(1);
        let h = WATERFALL_ROWS;
        let mut pixels = vec![Color32::BLACK; w * h];
        for (y, row_data) in self.rows.iter().enumerate() {
            let row_view = extract_view_window(
                row_data,
                view.row_rate_hz,
                view.view_span_hz,
                view.pan_offset_hz,
            );
            let base = y * w;
            for (x, &db) in row_view.iter().enumerate() {
                if x < w {
                    pixels[base + x] = db_to_colour(db, self.ref_db, self.range_db);
                }
            }
        }
        let image = egui::ColorImage::new([w, h], pixels);
        match &mut self.texture {
            Some(tex) => tex.set(image, egui::TextureOptions::LINEAR),
            none => {
                *none = Some(ctx.load_texture("waterfall", image, egui::TextureOptions::LINEAR));
            }
        }
    }

    fn history_panel(&mut self, ui: &mut egui::Ui) {
        section_heading(ui, "Callsign log (10 min)");
        let center_hz = self.center_khz * 1000.0;
        let annotations: Vec<_> = self.slow.annotations().iter().cloned().collect();
        if annotations.is_empty() {
            ui.label(
                egui::RichText::new("Decoded callsigns appear here when skimmer is on.")
                    .small()
                    .color(MUTED),
            );
            return;
        }
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for ann in annotations {
                    let age = ann.at.elapsed();
                    let freq_khz = center_hz + ann.offset_hz as f64;
                    let age_s = age.as_secs();
                    let age_txt = if age_s < 60 {
                        format!("{age_s}s ago")
                    } else {
                        format!("{}m ago", age_s / 60)
                    };
                    let frame = egui::Frame::new()
                        .fill(Color32::from_rgb(32, 38, 52))
                        .corner_radius(egui::CornerRadius::same(6))
                        .inner_margin(8.0)
                        .stroke(egui::Stroke::new(1.0, Color32::from_rgb(55, 65, 85)));
                    frame.show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(&ann.label).strong().color(OK));
                            ui.label(
                                egui::RichText::new(format!("{freq_khz:.1} kHz"))
                                    .monospace()
                                    .small(),
                            );
                            ui.label(
                                egui::RichText::new(format!("+{:.0} dB", ann.snr_db))
                                    .small()
                                    .color(MUTED),
                            );
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui
                                    .small_button("Tune")
                                    .on_hover_text("Tune receiver to this spot")
                                    .clicked()
                                {
                                    self.tune_to_hz(freq_khz);
                                }
                                ui.label(egui::RichText::new(age_txt).small().color(MUTED));
                            });
                        });
                    });
                    ui.add_space(4.0);
                }
            });
    }

    fn connection_popup(&mut self, ctx: &egui::Context) {
        if !self.show_connection_drawer {
            return;
        }
        let screen = ctx.content_rect();
        let win_h = (screen.height() * 0.72).clamp(280.0, 520.0);
        egui::Window::new("Connection")
            .id(egui::Id::new("connection_popup"))
            .collapsible(false)
            .resizable(true)
            .default_width(540.0)
            .default_height(win_h)
            .default_pos([screen.left() + 10.0, screen.top() + 34.0])
            .max_height(win_h)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    section_heading(ui, "Connection");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Close").clicked() {
                            self.show_connection_drawer = false;
                        }
                    });
                });
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        self.connection_card(ui);
                    });
            });
    }

    fn status_banner(&mut self, ui: &mut egui::Ui) {
        let conn_label = match &self.conn_state {
            ConnState::Streaming if self.connection_unstable() => "UNSTABLE".to_string(),
            ConnState::Streaming => "STREAMING".to_string(),
            ConnState::Reconnecting { attempt, retry_in_s } => {
                format!("RECONNECT #{attempt} ({retry_in_s:.0}s)")
            }
            ConnState::Connecting { .. } => "CONNECTING".to_string(),
            _ => "OFFLINE".to_string(),
        };
        let conn_color = match &self.conn_state {
            ConnState::Streaming if !self.connection_unstable() => OK,
            ConnState::Disconnected => MUTED,
            _ => WARN,
        };
        ui.horizontal(|ui| {
            let badge_resp = clickable_badge(ui, &conn_label, conn_color)
                .on_hover_text("Click to open/close connection settings");
            if badge_resp.clicked() {
                self.show_connection_drawer = !self.show_connection_drawer;
            }
            ui.separator();
            ui.label(egui::RichText::new("RX").small().color(MUTED));
            ui.add(
                egui::DragValue::new(&mut self.center_khz)
                    .speed(0.05)
                    .suffix(" kHz"),
            );
            ui.separator();
            ui.label(
                egui::RichText::new(format!("listen {:.0} Hz", self.listen_offset_hz()))
                    .small()
                    .color(MUTED),
            );
            ui.label(
                egui::RichText::new(format!("SNR {:.0} dB", self.last_snr_db))
                    .small()
                    .color(MUTED),
            );
            ui.separator();
            ui.label(format!("{:.0} kS/s", self.stats.effective_sps / 1000.0));
            if self.stats.dropped > 0 {
                ui.separator();
                ui.colored_label(WARN, format!("drops {}", self.stats.dropped));
            }
            if let Some(rssi) = self.stats.rssi_dbm {
                ui.separator();
                ui.label(format!("{rssi:.0} dBm"));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button("F11")
                    .on_hover_text("Toggle fullscreen (F11)")
                    .clicked()
                {
                    let on = ui.input(|i| i.viewport().fullscreen.unwrap_or(false));
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::Fullscreen(!on));
                }
                if matches!(
                    self.conn_state,
                    ConnState::Streaming | ConnState::Reconnecting { .. } | ConnState::Connecting { .. }
                ) && ui.button("Disconnect").on_hover_text("Stop streaming").clicked()
                {
                    self.engine.send(EngineCommand::Disconnect);
                }
                ui.separator();
                ui.toggle_value(&mut self.show_right, "Right");
                ui.toggle_value(&mut self.show_history, "Spots");
                ui.toggle_value(&mut self.show_left, "Left");
                ui.toggle_value(&mut self.show_console, "Log");
                if self.connection_unstable() {
                    ui.separator();
                    ui.colored_label(WARN, "connection unstable");
                }
            });
        });
        if let Some(err) = &self.last_error {
            if matches!(self.conn_state, ConnState::Reconnecting { .. }) {
                ui.colored_label(WARN, err);
            }
        }
    }

    fn left_panel(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                self.frequency_card(ui);
                self.filter_pipeline_card(ui);
                self.notch_card(ui);
            });
    }

    fn right_panel(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                self.spot_display_card(ui);
                self.cw_demod_card(ui);
                collapsible_section(ui, "skimmer-settings", "Skimmer settings", false, |ui| {
                    self.skimmer_settings_body(ui);
                });
                collapsible_section(ui, "audio", "Audio", false, |ui| {
                    self.audio_card_body(ui);
                });
                self.display_section(ui);
                self.performance_section(ui);

                ui.add_space(4.0);
                section_hint(
                    ui,
                    "Z zero-beat · L lock · N notch · B NB · R NR · A AGC · [ ] width · F11 fullscreen",
                );
            });
    }

    fn spot_display_card(&mut self, ui: &mut egui::Ui) {
        section_card(ui, |ui| {
            section_heading(ui, "Spots");
            ui.horizontal(|ui| {
                toggle(ui, &mut self.skimmer_enabled, "Skimmer on");
                if ui.button("Clear").on_hover_text("Clear all spots").clicked() {
                    self.clear_spots();
                }
                let n = self.frame_visible_spots.len();
                ui.label(
                    egui::RichText::new(format!("{n} shown · {} decoded", self.skimmer_spots.len()))
                        .small()
                        .color(MUTED),
                );
            });
            if !self.skimmer_enabled {
                ui.colored_label(MUTED, "Enable skimmer to decode callsigns on the band.");
            }
            scroll_slider_f32(ui, &mut self.min_spot_snr, 0.0..=40.0, "Table min SNR");
            scroll_slider_f32(ui, &mut self.spot_max_age_secs, 0.0..=300.0, "Max age (s, 0=all)");
            let mut label_lim = self.spot_label_limit as i32;
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Plot labels").small().color(MUTED));
                ui.add(egui::DragValue::new(&mut label_lim).range(8..=80).speed(1));
            });
            self.spot_label_limit = label_lim as usize;
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Call filter").small().color(MUTED));
                ui.add(
                    egui::TextEdit::singleline(&mut self.spot_callsign_filter)
                        .desired_width(100.0)
                        .hint_text("e.g. G or DL"),
                );
            });
            toggle(ui, &mut self.spot_cq_only, "CQ only");
            toggle(ui, &mut self.spot_hide_heard_labels, "Hide unconfirmed on plot");
            ui.checkbox(&mut self.continent_filter, "Filter by continent");
            if self.continent_filter {
                ui.horizontal_wrapped(|ui| {
                    for c in Continent::ALL {
                        let idx = continent_index(c);
                        let on = self.show_continents[idx];
                        if ui.selectable_label(on, c.code()).clicked() {
                            self.show_continents[idx] = !on;
                        }
                    }
                });
            }
            if self.continent_filter && !self.show_continents.iter().any(|&on| on) {
                ui.colored_label(WARN, "All continents off — no spots will match");
            }
            ui.separator();
            self.spot_table(ui);
        });
    }

    fn connection_card(&mut self, ui: &mut egui::Ui) {
        if self.connection_unstable() {
                ui.add_space(4.0);
                ui.colored_label(
                    WARN,
                    "Link slow or reconnecting — spectrum may be frozen; tuning is kept.",
                );
                if let Some(err) = &self.last_error {
                    ui.label(
                        egui::RichText::new(err)
                            .small()
                            .color(Color32::from_rgb(248, 113, 113)),
                    );
                }
            }

            ui.add_space(6.0);
            egui::Grid::new("connect_form")
                .num_columns(2)
                .spacing([8.0, 6.0])
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("Source").small().color(MUTED));
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut self.form_kind, SourceKind::Kiwi, "KiwiSDR");
                        #[cfg(feature = "airspy")]
                        ui.selectable_value(&mut self.form_kind, SourceKind::Airspy, "Airspy");
                    });
                    ui.end_row();

                    if self.form_kind == SourceKind::Kiwi {
                        ui.label(egui::RichText::new("Host").small().color(MUTED));
                        ui.add(
                            egui::TextEdit::singleline(&mut self.form_host)
                                .hint_text("kiwi.example.com")
                                .desired_width(f32::INFINITY),
                        );
                        ui.end_row();

                        ui.label(egui::RichText::new("Port").small().color(MUTED));
                        ui.add(egui::DragValue::new(&mut self.form_port).range(1..=65535));
                        ui.end_row();
                    }

                    ui.label(egui::RichText::new("Center").small().color(MUTED));
                    ui.add(
                        egui::DragValue::new(&mut self.form_center_mhz)
                            .range(0.0..=60.0)
                            .speed(0.001)
                            .suffix(" MHz")
                            .fixed_decimals(3),
                    );
                    ui.end_row();
                });

            let connecting = matches!(
                self.conn_state,
                ConnState::Connecting { .. } | ConnState::Reconnecting { .. }
            );
            ui.horizontal(|ui| {
                let can_connect = {
                    #[cfg(feature = "airspy")]
                    {
                        self.form_kind == SourceKind::Airspy || !self.form_host.trim().is_empty()
                    }
                    #[cfg(not(feature = "airspy"))]
                    {
                        !self.form_host.trim().is_empty()
                    }
                };
                if ui
                    .add_enabled(can_connect && !connecting, egui::Button::new("Connect"))
                    .clicked()
                {
                    self.connect_now();
                }
                if connecting && ui.button("Cancel").clicked() {
                    self.engine.send(EngineCommand::Disconnect);
                }
                if matches!(self.conn_state, ConnState::Streaming | ConnState::Reconnecting { .. } | ConnState::Connecting { .. })
                    && ui.button("Disconnect").clicked()
                {
                    self.engine.send(EngineCommand::Disconnect);
                }
            });

            if let Some(err) = &self.last_error {
                if matches!(
                    self.conn_state,
                    ConnState::Reconnecting { .. } | ConnState::Connecting { .. }
                ) {
                    ui.colored_label(WARN, err);
                }
            }

            if self.form_kind == SourceKind::Kiwi {
                ui.add_space(6.0);
                if self.kiwi_directory_rx.is_some() {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(
                            egui::RichText::new("Loading public KiwiSDRs…")
                                .small()
                                .color(MUTED),
                        );
                    });
                } else if !self.kiwi_nearby.is_empty() {
                    let header = if let Some(geo) = &self.kiwi_geo {
                        format!(
                            "Nearby ({}, sorted by distance)",
                            geo.country_code
                        )
                    } else {
                        "Public KiwiSDRs (sorted by distance)".to_string()
                    };
                    ui.label(egui::RichText::new(header).small().color(MUTED));
                    section_hint(
                        ui,
                        "Receivers marked FULL have no free slots — pick one with open users.",
                    );
                    let mut nearby = self.kiwi_nearby.clone();
                    nearby.sort_by(|a, b| {
                        let af = a.users >= a.users_max;
                        let bf = b.users >= b.users_max;
                        af.cmp(&bf)
                            .then_with(|| a.distance_km.partial_cmp(&b.distance_km).unwrap_or(std::cmp::Ordering::Equal))
                    });
                    egui::ScrollArea::vertical()
                        .max_height(160.0)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            for rx in nearby {
                                let full = rx.users >= rx.users_max;
                                let dist = if rx.distance_km > 0.0 {
                                    format!("{:.0} km · ", rx.distance_km)
                                } else {
                                    String::new()
                                };
                                let users = if full {
                                    format!("FULL {}/{}", rx.users, rx.users_max)
                                } else {
                                    format!("{}/{} users", rx.users, rx.users_max)
                                };
                                let label = format!(
                                    "{}:{} · {}{} · {}",
                                    rx.host,
                                    rx.port,
                                    dist,
                                    users,
                                    rx.location,
                                );
                                let btn = egui::Button::new(
                                    egui::RichText::new(label)
                                        .small()
                                        .color(if full { MUTED } else { Color32::WHITE }),
                                );
                                if ui
                                    .add_enabled(!full, btn)
                                    .on_hover_text(if full {
                                        "All slots busy on this Kiwi"
                                    } else {
                                        "Connect to this receiver"
                                    })
                                    .clicked()
                                {
                                    self.form_host = rx.host;
                                    self.form_port = rx.port;
                                    self.connect_now();
                                }
                            }
                        });
                    if ui.small_button("Refresh list").clicked() {
                        self.start_kiwi_directory_fetch(true);
                    }
                } else if let Some(err) = &self.kiwi_directory_error {
                    ui.colored_label(WARN, err);
                    if ui.small_button("Retry directory").clicked() {
                        self.kiwi_directory_error = None;
                        self.start_kiwi_directory_fetch(true);
                    }
                } else {
                    ui.label(
                        egui::RichText::new("No receivers in cache — use Refresh")
                            .small()
                            .color(MUTED),
                    );
                    if ui.small_button("Refresh list").clicked() {
                        self.start_kiwi_directory_fetch(true);
                    }
                }
            }

            if !self.recent_hosts.is_empty() {
                ui.add_space(6.0);
                ui.label(egui::RichText::new("Recent").small().color(MUTED));
                let recents = self.recent_hosts.clone();
                for req in recents {
                    if ui.button(req.label()).clicked() {
                        self.form_kind = req.kind;
                        self.form_host = req.host.clone();
                        self.form_port = req.port;
                        self.form_center_mhz = req.center_hz / 1e6;
                        self.connect_now();
                    }
                }
            }

            ui.add_space(6.0);
            stat_row(ui, "Effective", format!("{:.1} kS/s", self.stats.effective_sps / 1000.0));
            stat_row(ui, "Dropped", self.stats.dropped.to_string());
            if let Some(rssi) = self.stats.rssi_dbm {
                stat_row(ui, "S-meter", format!("{rssi:.1} dBm"));
            }
    }

    fn display_section(&mut self, ui: &mut egui::Ui) {
        collapsible_section(ui, "display", "Display", false, |ui| {
            scroll_slider_f32(ui, &mut self.plot_view.zoom, 0.04..=1.0, "Zoom");
            if ui.small_button("Full span (F)").clicked() {
                self.plot_view.zoom = 1.0;
                self.plot_view.pan_offset_hz = 0.0;
            }
            let floor_db = self.ref_db - self.range_db;
            ui.label(
                egui::RichText::new(format!(
                    "Floor {:.0} dB · Ref {:.0} dB · Range {:.0} dB",
                    floor_db, self.ref_db, self.range_db
                ))
                .small()
                .color(MUTED),
            );
            ui.horizontal(|ui| {
                if ui
                    .button("Auto levels")
                    .on_hover_text("Set Ref/Range once from the live spectrum")
                    .clicked()
                {
                    self.display_levels_initialized = false;
                    self.update_display_levels();
                }
                ui.toggle_value(
                    &mut self.display_auto_track,
                    "Track continuously",
                )
                .on_hover_text("Keep adjusting Ref/Range as the band changes");
            });
            if scroll_slider_f32(ui, &mut self.ref_db, -120.0..=20.0, "Ref dB").changed() {
                self.display_levels_initialized = true;
                self.display_auto_track = false;
            }
            if scroll_slider_f32(ui, &mut self.range_db, 12.0..=80.0, "Range dB").changed() {
                self.display_levels_initialized = true;
                self.display_auto_track = false;
            }
            scroll_slider_f32(ui, &mut self.smooth_alpha, 0.05..=0.45, "Smoothing");
        });
    }

    fn passband_max_hz(&self) -> f32 {
        if self.filter_wide {
            CW_PASSBAND_MAX_HZ
        } else {
            CW_PASSBAND_NARROW_MAX_HZ
        }
    }

    fn performance_section(&mut self, ui: &mut egui::Ui) {
        collapsible_section(ui, "perf", "Performance", false, |ui| {
            ui.checkbox(&mut self.fft_auto, "Auto FFT size (wideband)");
            if self.fft_auto {
                let rate = self.stats.spectrum_rate;
                let bin = rate / self.stats.spectrum_fft.max(1) as f32;
                let zoom_note = if self.stats.spectrum_zoomed {
                    format!(" ×{} zoom", self.stats.spectrum_decim)
                } else {
                    String::new()
                };
                stat_row(
                    ui,
                    "FFT",
                    format!(
                        "{} @ {:.1} kS/s (~{bin:.1} Hz/bin){zoom_note}",
                        self.stats.spectrum_fft,
                        rate / 1000.0
                    ),
                );
            } else {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("FFT").small().color(MUTED));
                    for n in [2048usize, 4096, 8192, 16_384, 32_768, 65_536] {
                        if ui.selectable_label(self.fft_size == n, n.to_string()).clicked() {
                            self.fft_size = n;
                        }
                    }
                });
            }

            let mut dec = self.cw.decimation as i32;
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Decimation").small().color(MUTED));
                ui.add(egui::DragValue::new(&mut dec).range(0..=64).speed(1));
                ui.label(egui::RichText::new(if dec == 0 { "auto" } else { "manual" }).small().color(MUTED));
            });
            self.cw.decimation = dec.max(0) as u32;
            let factor = if self.cw.decimation == 0 {
                decimation_factor(self.sample_rate)
            } else {
                self.cw.decimation as usize
            }
            .max(1);
            stat_row(ui, "Audio rate", format!("{:.1} kHz", self.sample_rate / factor as f32 / 1000.0));

            let mut fps = self.target_fps as f32;
            if scroll_slider_f32(ui, &mut fps, 10.0..=60.0, "Target FPS").changed() {
                self.target_fps = fps.round() as u32;
            }

            ui.separator();
            stat_row(ui, "IQ / pump", self.stats.last_drain.to_string());
            stat_row(ui, "Decoders", self.skimmer_channels.to_string());
            if let Some(name) = &self.stats.audio_device {
                stat_row(ui, "Audio out", name.clone());
            }
        });
    }

    fn frequency_card(&mut self, ui: &mut egui::Ui) {
        section_card(ui, |ui| {
            section_heading(ui, "Operator");
            ui.horizontal_wrapped(|ui| {
                for (label, hz) in BAND_PRESETS {
                    let selected = (self.center_khz * 1000.0).round() == hz;
                    if ui.selectable_label(selected, label).clicked() {
                        self.center_khz = hz / 1000.0;
                    }
                }
            });
            scroll_drag_f64(ui, &mut self.center_khz, 0.0..=60_000.0, 0.05, " kHz");
            scroll_slider_f32(ui, &mut self.rit_hz, -800.0..=800.0, "RIT");
            ui.horizontal(|ui| {
                if ui.button("Zero-beat (Z)").on_hover_text("Snap listen passband to strongest carrier").clicked() {
                    self.zero_beat();
                }
                ui.checkbox(&mut self.pitch_lock, "Lock pitch (L)");
            });
        });
    }

    fn cw_demod_card(&mut self, ui: &mut egui::Ui) {
        section_card(ui, |ui| {
            section_heading(ui, "CW demod");
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new("BFO").small().color(MUTED));
                for (label, hz) in BFO_PRESETS {
                    if ui.selectable_label(self.cw.bfo_hz.round() == hz, label).clicked() {
                        self.cw.bfo_hz = hz;
                    }
                }
            });
            scroll_slider_f32(ui, &mut self.cw.bfo_hz, 300.0..=1_200.0, "BFO tone");
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.filter_wide, false, "CW (≤500 Hz)");
                ui.selectable_value(&mut self.filter_wide, true, "Wide (≤2 kHz)");
            });
            let bw_max = self.passband_max_hz();
            if self.cw.passband_hz > bw_max {
                self.cw.passband_hz = bw_max;
            }
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new("BW").small().color(MUTED));
                for (label, hz) in FILTER_PRESETS {
                    if hz > bw_max {
                        continue;
                    }
                    if ui.selectable_label(self.cw.passband_hz.round() == hz, label).clicked() {
                        self.cw.passband_hz = hz;
                    }
                }
            });
            scroll_slider_log_f32(
                ui,
                &mut self.cw.passband_hz,
                CW_PASSBAND_MIN_HZ..=bw_max,
                "Audio BW",
            );
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Shape").small().color(MUTED));
                window_choice(
                    ui,
                    &mut self.cw.window,
                    WindowKind::Gaussian,
                    "Gauss",
                    "Softest tone, gentle skirts — clean signals, minimal ringing",
                );
                window_choice(
                    ui,
                    &mut self.cw.window,
                    WindowKind::RaisedCosine,
                    "RaisedCos",
                    "Balanced default — good tone with moderate adjacent rejection",
                );
                window_choice(
                    ui,
                    &mut self.cw.window,
                    WindowKind::Blackman,
                    "Blackman",
                    "Steepest skirts — reject nearby QRM before narrowing bandwidth",
                );
            });
            let audio_rate = hfsdr::audio_sample_rate(self.sample_rate, self.cw.decimation);
            let delay_ms = channel_group_delay_ms(audio_rate, self.cw.passband_hz);
            ui.label(
                egui::RichText::new(format!("Filter delay ~{delay_ms:.0} ms (linear-phase FIR)"))
                    .small()
                    .color(MUTED),
            );
            section_hint(
                ui,
                "Narrow BW alone rarely beats QRM — try Blackman at 200–300 Hz, then manual notches on hets.",
            );
            section_hint(
                ui,
                "APF after demod sharpens tone without smearing dits. Keep NR light (stutters if heavy).",
            );
            section_hint(ui, "Ctrl+scroll on plot: BW · drag cyan edges");
        });
    }

    fn filter_pipeline_card(&mut self, ui: &mut egui::Ui) {
        collapsible_section(ui, "pipeline", "QRM tools", true, |ui| {
            section_hint(ui, "Each stage toggles independently (stackable).");

            ui.checkbox(&mut self.cw.noise_blanker.enabled, "Noise blanker (B) — impulse QRN");
            if self.cw.noise_blanker.enabled {
                scroll_slider_f32(ui, &mut self.cw.noise_blanker.threshold, 2.0..=12.0, "NB threshold");
                let mut width = self.cw.noise_blanker.width as f32;
                scroll_slider_f32(ui, &mut width, 1.0..=30.0, "NB recovery");
                self.cw.noise_blanker.width = width.round() as usize;
                section_hint(ui, "Soft limiter — attenuates lightning/ignition without muting CW.");
            }

            ui.separator();
            ui.checkbox(&mut self.cw.auto_notch.enabled, "Auto-notch (N) — guard-aware");
            if self.cw.auto_notch.enabled {
                scroll_slider_f32(ui, &mut self.cw.auto_notch.guard_hz, 60.0..=300.0, "Guard ±Hz");
                scroll_slider_f32(ui, &mut self.cw.auto_notch.rate, 0.002..=0.1, "Adapt rate");
            }

            ui.separator();
            ui.checkbox(&mut self.cw.apf.enabled, "Audio peak filter — boost at pitch");
            if self.cw.apf.enabled {
                scroll_slider_f32(ui, &mut self.cw.apf.width_hz, 40.0..=300.0, "APF width");
                scroll_slider_f32(ui, &mut self.cw.apf.gain, 0.2..=4.0, "APF gain");
            }

            ui.separator();
            ui.checkbox(&mut self.cw.noise_reduction.enabled, "Noise reduction (R) — light LMS");
            if self.cw.noise_reduction.enabled {
                scroll_slider_f32(ui, &mut self.cw.noise_reduction.level, 0.0..=0.5, "NR level");
                section_hint(ui, "Keep below ~0.4 — higher values smear keying and sound underwater.");
            }

            ui.separator();
            ui.checkbox(&mut self.cw.agc.enabled, "AGC (A)");
            if self.cw.agc.enabled {
                scroll_slider_f32(ui, &mut self.cw.agc.attack_ms, 1.0..=20.0, "Attack ms");
                scroll_slider_f32(ui, &mut self.cw.agc.decay_ms, 20.0..=600.0, "Decay ms");
                scroll_slider_f32(ui, &mut self.cw.agc.target, 0.05..=0.6, "Target");
            } else {
                scroll_slider_f32(ui, &mut self.cw.agc.manual_gain, 0.1..=16.0, "Manual gain");
            }
            if self.is_kiwi {
                ui.checkbox(&mut self.agc_rf_on, "Kiwi RF AGC");
            }

            ui.separator();
            scroll_slider_f32(ui, &mut self.cw.squelch, 0.0..=0.08, "Squelch");
        });
    }

    fn notch_card(&mut self, ui: &mut egui::Ui) {
        collapsible_section(ui, "notches", "Manual notches", false, |ui| {
            section_hint(ui, "Steer multiple notches onto hets in the passband.");
            if let Some(hover) = self.hover_offset_hz {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(format!("Cursor {hover:.0} Hz")).small().color(MUTED));
                    if ui.small_button("Notch at cursor").clicked() {
                        if let Some(slot) = self.cw.notches.iter_mut().find(|n| !n.enabled) {
                            slot.enabled = true;
                            slot.offset_hz = hover as f32;
                        }
                    }
                });
            }
            for idx in 0..MAX_NOTCHES {
                let notch = &mut self.cw.notches[idx];
                ui.horizontal(|ui| {
                    ui.checkbox(&mut notch.enabled, format!("#{}", idx + 1));
                });
                if notch.enabled {
                    scroll_slider_f32(ui, &mut notch.offset_hz, -5_000.0..=5_000.0, "Offset");
                    scroll_slider_f32(ui, &mut notch.width_hz, 10.0..=200.0, "Width");
                }
            }
        });
    }

    fn skimmer_settings_body(&mut self, ui: &mut egui::Ui) {
        if self.skimmer_enabled {
            stat_row(ui, "Decoders", self.skimmer_channels.to_string());
        }
        self.scp_section(ui);

        section_heading(ui, "Decoder & channel DSP");
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Algorithm").small().color(MUTED));
                if ui
                    .selectable_label(
                        self.skimmer.decoder == SkimmerDecoderKind::Bigram,
                        "Bigram beam",
                    )
                    .clicked()
                {
                    self.skimmer.decoder = SkimmerDecoderKind::Bigram;
                }
                if ui
                    .selectable_label(
                        self.skimmer.decoder == SkimmerDecoderKind::Adaptive,
                        "Adaptive",
                    )
                    .clicked()
                {
                    self.skimmer.decoder = SkimmerDecoderKind::Adaptive;
                }
            });
            section_hint(
                ui,
                "Bigram: best copy on pileups · Adaptive: lighter CPU",
            );
            scroll_slider_f32(ui, &mut self.skimmer.min_snr_db, 6.0..=30.0, "Peak min SNR");
            scroll_slider_f32(ui, &mut self.skimmer.min_decode_snr_db, 6.0..=40.0, "Decode min SNR");
            scroll_slider_f32(ui, &mut self.skimmer.decode_gate_ms, 20.0..=500.0, "Key gate ms");
            scroll_slider_f32(ui, &mut self.skimmer.bucket_hz, 20.0..=200.0, "Bucket Hz");
            let mut sep = self.skimmer.min_separation_bins as i32;
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Peak separation").small().color(MUTED));
                ui.add(egui::DragValue::new(&mut sep).range(1..=32).speed(1));
                ui.label(egui::RichText::new("bins").small().color(MUTED));
            });
            self.skimmer.min_separation_bins = sep as usize;
            let mut max_ch = self.skimmer.max_channels as i32;
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Max decoders").small().color(MUTED));
                ui.add(egui::DragValue::new(&mut max_ch).range(4..=64).speed(1));
            });
            self.skimmer.max_channels = max_ch as usize;
            scroll_slider_f32(
                ui,
                &mut self.skimmer.lpf_cutoff_hz,
                40.0..=800.0,
                "Channel LPF Hz",
            );
            scroll_slider_log_f32(
                ui,
                &mut self.skimmer.target_audio_rate_hz,
                4_000.0..=48_000.0,
                "Target audio rate",
            );
            scroll_slider_f32(
                ui,
                &mut self.skimmer.decoder_params.initial_wpm,
                8.0..=60.0,
                "Initial WPM",
            );
            if self.skimmer.decoder == SkimmerDecoderKind::Bigram {
                let mut beam = self.skimmer.decoder_params.beam_width as i32;
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Beam width").small().color(MUTED));
                    ui.add(egui::DragValue::new(&mut beam).range(1..=64).speed(1));
                });
                self.skimmer.decoder_params.beam_width = beam as usize;
            }
            scroll_slider_f32(
                ui,
                &mut self.skimmer.decoder_params.envelope.thr_low,
                0.05..=0.9,
                "Key thr low",
            );
            scroll_slider_f32(
                ui,
                &mut self.skimmer.decoder_params.envelope.thr_high,
                0.1..=0.99,
                "Key thr high",
            );
            if self.skimmer.decoder_params.envelope.thr_high
                <= self.skimmer.decoder_params.envelope.thr_low
            {
                self.skimmer.decoder_params.envelope.thr_high =
                    self.skimmer.decoder_params.envelope.thr_low + 0.05;
            }
            scroll_slider_f32(
                ui,
                &mut self.skimmer.channel_timeout_secs,
                1.0..=120.0,
                "Channel timeout (s)",
            );
            scroll_slider_f32(
                ui,
                &mut self.skimmer.spot_store_max_age_secs,
                0.0..=600.0,
                "Store max age (s, 0=keep)",
            );
            let mut max_txt = self.skimmer.decoder_params.max_text_chars as i32;
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Max decode chars").small().color(MUTED));
                ui.add(egui::DragValue::new(&mut max_txt).range(16..=256).speed(1));
            });
            self.skimmer.decoder_params.max_text_chars = max_txt as usize;
            toggle(
                ui,
                &mut self.skimmer.require_scp,
                "Require MASTER.SCP match",
            );
    }

    fn spot_table(&mut self, ui: &mut egui::Ui) {
        let spots = &self.frame_visible_spots;
        let sort = &mut self.spot_sort;
        let mut tune_to: Option<f64> = None;
        TableBuilder::new(ui)
            .striped(true)
            .sense(egui::Sense::click())
            .max_scroll_height(300.0)
            .column(Column::exact(24.0))
            .column(Column::remainder().at_least(56.0))
            .column(Column::exact(72.0))
            .column(Column::exact(40.0))
            .column(Column::exact(40.0))
            .column(Column::exact(36.0))
            .header(18.0, |mut header| {
                header.col(|_| {});
                header.col(|ui| {
                    if ui.button("Call").clicked() {
                        *sort = SpotSort::Callsign;
                    }
                });
                header.col(|ui| {
                    if ui.button("kHz").clicked() {
                        *sort = SpotSort::Frequency;
                    }
                });
                header.col(|ui| {
                    if ui.button("dB").clicked() {
                        *sort = SpotSort::SnrDesc;
                    }
                });
                header.col(|ui| {
                    ui.label(egui::RichText::new("wpm").small().color(MUTED));
                });
                header.col(|ui| {
                    if ui.button("Age").clicked() {
                        *sort = SpotSort::LastHeard;
                    }
                });
            })
            .body(|mut body| {
                for spot in spots {
                    body.row(18.0, |mut row| {
                        let (glyph, color) = match spot.kind {
                            SpotKind::CallingCq => ("CQ", WARN),
                            SpotKind::Answering => ("→", OK),
                            SpotKind::Heard => ("·", MUTED),
                        };
                        row.col(|ui| {
                            ui.label(egui::RichText::new(glyph).monospace().color(color));
                        });
                        row.col(|ui| {
                            let call = spot.callsign.as_deref().unwrap_or("…");
                            ui.label(egui::RichText::new(call).monospace().color(color));
                        });
                        row.col(|ui| {
                            ui.label(format!("{:.1}", spot.frequency_hz / 1000.0));
                        });
                        row.col(|ui| {
                            ui.label(format!("{:.0}", spot.snr_db));
                        });
                        row.col(|ui| {
                            ui.label(format!("{:.0}", spot.wpm));
                        });
                        row.col(|ui| {
                            let secs = spot.age().as_secs();
                            ui.label(
                                egui::RichText::new(if secs < 60 {
                                    format!("{secs}s")
                                } else {
                                    format!("{}m", secs / 60)
                                })
                                .small()
                                .color(MUTED),
                            );
                        });
                        if row.response().clicked() {
                            tune_to = Some(spot.frequency_hz);
                        }
                    });
                }
            });
        if let Some(hz) = tune_to {
            self.tune_to_hz(hz);
        }
    }

    fn audio_card_body(&mut self, ui: &mut egui::Ui) {
        if self.audio_devices.is_empty() {
                ui.colored_label(WARN, "No output devices found");
            } else {
                let selected = self
                    .audio_devices
                    .get(self.selected_audio_device)
                    .map(String::as_str)
                    .unwrap_or("");
                egui::ComboBox::from_label("Output device")
                    .selected_text(selected)
                    .show_ui(ui, |ui| {
                        for (idx, name) in self.audio_devices.iter().enumerate() {
                            ui.selectable_value(&mut self.selected_audio_device, idx, name);
                        }
                    });
                if ui.small_button("Refresh devices").clicked() {
                    self.audio_devices = AudioOutput::list_output_devices();
                    if self.selected_audio_device >= self.audio_devices.len() {
                        self.selected_audio_device = 0;
                    }
                    self.last_audio_device = usize::MAX;
                }
            }
            ui.checkbox(&mut self.audio_enabled, "Play on speakers (Space)");
            scroll_slider_f32(ui, &mut self.volume, 0.0..=4.0, "Volume");
            if let Some(name) = &self.stats.audio_device {
                stat_row(ui, "Active", name.clone());
                stat_row(ui, "Rate", format!("{} Hz", self.stats.audio_rate));
            } else {
                ui.colored_label(WARN, "No output device open");
            }
    }

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
                            "Not connected — click OFFLINE in the status bar to pick a receiver",
                        );
                    }
                    ConnState::Streaming => {}
                }
            });
            ui.add_space(4.0);
        }

        self.plot_view.clamp_pan(self.sample_rate);
        let view = self.spectrum_view();
        let trace = display_trace(
            &self.latest,
            &mut self.smoothed_trace,
            view.row_rate_hz,
            view.view_span_hz,
            view.pan_offset_hz,
            self.smooth_alpha,
        );

        let mut plot_actions = Vec::new();
        let tune_preview_offset_hz = self.tune_preview_offset_hz.unwrap_or(0.0);
        let listen_center_hz = self.listen_offset_hz();
        let notches = self.enabled_notches();
        let labels = if self.skimmer_enabled {
            self.spot_labels(self.center_khz * 1000.0)
        } else {
            Vec::new()
        };

        let bw_max = self.passband_max_hz();
        let mut params = crate::widgets::PlotParams {
            sample_rate: self.sample_rate,
            center_freq_hz: self.center_khz * 1000.0,
            passband_hz: self.cw.passband_hz,
            passband_min_hz: CW_PASSBAND_MIN_HZ,
            passband_max_hz: bw_max,
            filter_editable: true,
            listen_center_hz,
            tune_preview_offset_hz,
            notches: &notches,
            labels: &labels,
            trace: &trace,
            ref_db: self.ref_db,
            range_db: self.range_db,
            height: 200.0,
        };

        let (_, spec_actions) = self.spectrum_widget.show(
            ui,
            &mut self.plot_interaction,
            &mut self.plot_view,
            &params,
            &mut self.hover_offset_hz,
        );
        plot_actions.extend(spec_actions);

        ui.add_space(4.0);

        if self.texture.is_some() {
            let tex = self.texture.clone().unwrap();
            params.trace = &[];
            let wf_actions = self.waterfall_widget.show(
                ui,
                &mut self.plot_interaction,
                &mut self.plot_view,
                &tex,
                &params,
                &mut self.hover_offset_hz,
            );
            plot_actions.extend(wf_actions);
        } else {
            ui.allocate_space(egui::vec2(ui.available_width(), ui.available_height().max(120.0)));
            ui.centered_and_justified(|ui| {
                let msg = if matches!(self.conn_state, ConnState::Disconnected) {
                    "Connect to a receiver to see live spectrum"
                } else {
                    "Waiting for IQ data…"
                };
                ui.label(egui::RichText::new(msg).color(MUTED));
            });
        }

        self.apply_plot_actions(plot_actions);
    }
}

fn window_choice(
    ui: &mut egui::Ui,
    current: &mut WindowKind,
    kind: WindowKind,
    label: &str,
    tip: &str,
) {
    let r = ui.selectable_label(*current == kind, label);
    if r.clicked() {
        *current = kind;
    }
    r.on_hover_text(tip);
}

fn window_to_u8(w: WindowKind) -> u8 {
    match w {
        WindowKind::Gaussian => 0,
        WindowKind::RaisedCosine => 1,
        WindowKind::Blackman => 2,
    }
}

fn window_from_u8(v: u8) -> WindowKind {
    match v {
        1 => WindowKind::RaisedCosine,
        2 => WindowKind::Blackman,
        _ => WindowKind::Gaussian,
    }
}

fn spot_sort_to_u8(s: SpotSort) -> u8 {
    match s {
        SpotSort::SnrDesc => 0,
        SpotSort::Frequency => 1,
        SpotSort::LastHeard => 2,
        SpotSort::Callsign => 3,
    }
}

fn spot_sort_from_u8(v: u8) -> SpotSort {
    match v {
        1 => SpotSort::Frequency,
        2 => SpotSort::LastHeard,
        3 => SpotSort::Callsign,
        _ => SpotSort::SnrDesc,
    }
}

fn skimmer_decoder_to_u8(d: SkimmerDecoderKind) -> u8 {
    match d {
        SkimmerDecoderKind::Bigram => 0,
        SkimmerDecoderKind::Adaptive => 1,
    }
}

fn skimmer_decoder_from_u8(v: u8) -> SkimmerDecoderKind {
    match v {
        1 => SkimmerDecoderKind::Adaptive,
        _ => SkimmerDecoderKind::Bigram,
    }
}

fn skimmer_config_from_settings(s: &AppSettings) -> SkimmerConfig {
    use hfsdr::{DecoderParams, EnvelopeSettings};
    SkimmerConfig {
        bucket_hz: s.skimmer_bucket_hz,
        min_snr_db: s.skimmer_min_snr_db,
        min_decode_snr_db: s.skimmer_min_decode_snr_db,
        min_separation_bins: s.skimmer_min_separation_bins,
        max_channels: s.skimmer_max_channels.max(1),
        channel_timeout_secs: s.skimmer_channel_timeout_secs,
        spot_store_max_age_secs: s.skimmer_store_max_age_secs,
        source_label: "rx".to_string(),
        require_scp: s.scp_require,
        decoder: skimmer_decoder_from_u8(s.skimmer_decoder),
        lpf_cutoff_hz: s.skimmer_lpf_cutoff_hz,
        target_audio_rate_hz: s.skimmer_target_audio_rate_hz,
        decode_gate_ms: s.skimmer_decode_gate_ms,
        decoder_params: DecoderParams {
            initial_wpm: s.skimmer_initial_wpm,
            beam_width: s.skimmer_beam_width.max(1),
            envelope: EnvelopeSettings {
                thr_low: s.skimmer_thr_low,
                thr_high: s.skimmer_thr_high,
                min_span_fraction: EnvelopeSettings::default().min_span_fraction,
            },
            max_text_chars: s.skimmer_max_decode_chars.max(16),
        },
    }
    .clamped()
}


impl eframe::App for WaterfallApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if !self.themed {
            apply(&ctx);
            self.themed = true;
        }

        if let Some(req) = self.pending_connect.take() {
            self.form_kind = req.kind;
            self.form_host = req.host.clone();
            self.form_port = req.port;
            self.form_center_mhz = req.center_hz / 1e6;
            log::info(format!("connecting to {}", req.label()));
            self.engine.send(EngineCommand::Connect(req));
        }

        self.poll_scp_download();
        self.poll_kiwi_directory();
        self.handle_shortcuts(&ctx);
        self.pump_engine();
        self.frame_visible_spots = self.visible_spots();

        // Lazy texture rebuild: only when new rows arrived or the view changed.
        let view = self.spectrum_view();
        let pan_track = if self.stats.spectrum_zoomed {
            self.plot_view.pan_offset_hz
        } else {
            view.pan_offset_hz
        };
        let view_changed = (view.view_span_hz - self.last_tex_span).abs() > 1.0
            || (pan_track - self.last_tex_pan).abs() > 1.0
            || (view.row_rate_hz - self.last_tex_row_rate).abs() > 1.0;
        if self.textures_dirty || view_changed {
            self.update_texture(&ctx);
            self.textures_dirty = false;
            self.last_tex_span = view.view_span_hz;
            self.last_tex_pan = pan_track;
            self.last_tex_row_rate = view.row_rate_hz;
        }

        egui::Panel::top("status").show_inside(ui, |ui| self.status_banner(ui));

        if self.show_left {
            egui::Panel::left("left")
                .resizable(true)
                .default_size(300.0)
                .show_inside(ui, |ui| self.left_panel(ui));
        }

        if self.show_right {
            egui::Panel::right("controls")
                .resizable(true)
                .default_size(330.0)
                .show_inside(ui, |ui| self.right_panel(ui));
        }

        if self.show_history {
            egui::Panel::bottom("history")
                .resizable(true)
                .default_size(150.0)
                .show_inside(ui, |ui| self.history_panel(ui));
        }

        if self.show_console {
            egui::Panel::bottom("console")
                .resizable(true)
                .default_size(160.0)
                .show_inside(ui, |ui| self.console_panel(ui));
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show_inside(ui, |ui| {
                self.central_panel(ui);
            });

        self.connection_popup(&ctx);

        self.apply_radio_settings();
        self.autosave();

        let frame_ms = (1000 / self.target_fps.max(1)).max(8) as u64;
        ctx.request_repaint_after(Duration::from_millis(frame_ms));
    }

    fn on_exit(&mut self) {
        self.current_settings().save();
    }
}
