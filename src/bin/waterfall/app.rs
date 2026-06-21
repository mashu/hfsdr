//! Waterfall application state and rendering.
//!
//! The UI thread owns no DSP: it pushes settings to the [`crate::engine`] worker,
//! drains spectrum rows / status / spots it publishes, renders, and repaints
//! lazily. Connection lifecycle (connect, slow/unstable warnings, auto-reconnect)
//! is driven by the engine and surfaced here.

use std::collections::HashSet;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

use eframe::egui;
use egui::Color32;
use egui_extras::{Column, TableBuilder};
use hfsdr::{
    decimation_factor, extract_view_window, strongest_offset_hz, Continent, ContinentResolver,
    CwChannelSettings, RowFold, SlowWaterfall, Spot, SpotKind, SpotSort, WindowKind, MAX_NOTCHES,
};

use crate::audio::AudioOutput;
use crate::colormap::db_to_colour;
use crate::controls::{scroll_drag_f64, scroll_slider_f32, scroll_slider_log_f32};
use crate::display_levels::estimate_levels;
use crate::engine::{
    ConnState, EngineCommand, EngineHandle, EngineParams, EngineStats, FFT_SIZE, WATERFALL_ROWS,
};
use crate::interaction::{PlotAction, PlotInteraction, PlotViewState};
use crate::interaction::{CW_PASSBAND_MAX_HZ, CW_PASSBAND_MIN_HZ};
use crate::settings::{AppSettings, NotchData};
use crate::source::{ConnectRequest, SourceKind};
use crate::theme::{
    apply, badge, collapsible_section, section_card, section_heading, section_hint, stat_row,
    toggle, MUTED, OK, WARN,
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
const FILTER_PRESETS: [(&str, f32); 4] = [("50", 50.0), ("100", 100.0), ("250", 250.0), ("500", 500.0)];

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

    ref_db: f32,
    range_db: f32,
    display_levels_initialized: bool,
    smooth_alpha: f32,
    waterfall_rows: usize,
    target_fps: u32,
    fft_size: usize,

    audio_devices: Vec<String>,
    selected_audio_device: usize,
    last_audio_device: usize,
    audio_enabled: bool,
    volume: f32,

    skimmer_enabled: bool,
    skimmer_channels: usize,
    skimmer_spots: Vec<Spot>,
    spot_sort: SpotSort,
    continent_filter: bool,
    show_continents: [bool; 7],
    min_spot_snr: f32,
    resolver: ContinentResolver,
    annotated: HashSet<String>,
    slow: SlowWaterfall,
    slow_texture: Option<egui::TextureHandle>,
    show_history: bool,
    show_left: bool,
    show_right: bool,

    recent_hosts: Vec<ConnectRequest>,
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
            ref_db: -70.0,
            range_db: 55.0,
            display_levels_initialized: false,
            smooth_alpha: SMOOTH_ALPHA,
            waterfall_rows: 0,
            target_fps: 30,
            fft_size: FFT_SIZE,
            audio_devices,
            selected_audio_device: 0,
            last_audio_device: 0,
            audio_enabled: true,
            volume: 1.0,
            skimmer_enabled: false,
            skimmer_channels: 0,
            skimmer_spots: Vec::new(),
            spot_sort: SpotSort::SnrDesc,
            continent_filter: false,
            show_continents: [true; 7],
            min_spot_snr: 0.0,
            resolver: ContinentResolver::new(),
            annotated: HashSet::new(),
            slow: SlowWaterfall::new(2.0, 600.0, RowFold::Peak),
            slow_texture: None,
            show_history: false,
            show_left: true,
            show_right: true,
            recent_hosts: Vec::new(),
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
        }

        app.last_settings_json =
            serde_json::to_string(&app.current_settings()).unwrap_or_default();
        app
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
        self.smooth_alpha = s.smooth_alpha;
        self.target_fps = s.target_fps.clamp(10, 60);
        self.fft_size = s.fft_size.clamp(256, 16_384);

        self.audio_enabled = s.audio_enabled;
        self.volume = s.volume;

        self.skimmer_enabled = s.skimmer_enabled;
        self.min_spot_snr = s.min_spot_snr;
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
            smooth_alpha: self.smooth_alpha,
            target_fps: self.target_fps,
            fft_size: self.fft_size,
            audio_enabled: self.audio_enabled,
            volume: self.volume,
            skimmer_enabled: self.skimmer_enabled,
            min_spot_snr: self.min_spot_snr,
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
        self.engine.set_params(EngineParams {
            cw: self.cw.clone(),
            audio_enabled: self.audio_enabled,
            volume: self.volume,
            skimmer_enabled: self.skimmer_enabled,
            fft_size: self.fft_size,
        });

        let (state, stats, spots, new_rows, latest, last_error) = self.engine.with_shared(|s| {
            let rows: Vec<Vec<f32>> = s.new_rows.drain(..).collect();
            (
                s.state.clone(),
                s.stats.clone(),
                s.spots.clone(),
                rows,
                s.latest.clone(),
                s.last_error.clone(),
            )
        });

        self.conn_state = state;
        self.stats = stats;
        self.last_error = last_error;
        self.skimmer_spots = spots;
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

        if !new_rows.is_empty() {
            for row in new_rows {
                self.slow.push_row(&row);
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
            self.maybe_init_display_levels();
        }

        self.apply_pitch_lock();
        if self.skimmer_enabled {
            self.annotate_new_cqs(self.center_khz * 1000.0);
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
                    self.cw.passband_hz = bw.clamp(CW_PASSBAND_MIN_HZ, CW_PASSBAND_MAX_HZ);
                }
            }
        }
    }

    fn annotate_new_cqs(&mut self, center_hz: f64) {
        for spot in &self.skimmer_spots {
            if spot.kind != SpotKind::CallingCq {
                continue;
            }
            let Some(call) = &spot.callsign else { continue };
            if self.annotated.insert(call.clone()) {
                let offset = (spot.frequency_hz - center_hz) as f32;
                self.slow.annotate(offset, format!("CQ {call}"), spot.snr_db);
            }
        }
        if self.annotated.len() > 512 {
            self.annotated.clear();
        }
    }

    fn maybe_init_display_levels(&mut self) {
        if self.display_levels_initialized {
            return;
        }
        let Some((ref_db, range_db)) = estimate_levels(&self.latest) else {
            return;
        };
        self.ref_db = ref_db;
        self.range_db = range_db;
        self.display_levels_initialized = true;
    }

    /// Snap tuning so the strongest signal near the cursor lands at the BFO pitch.
    fn zero_beat(&mut self) {
        let listen = self.listen_offset_hz() as f32;
        if let Some(peak) = strongest_offset_hz(&self.latest, self.sample_rate, listen, 400.0) {
            self.center_khz += (peak - listen) as f64 / 1000.0;
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
        if let Some(peak) = strongest_offset_hz(&self.latest, self.sample_rate, listen, 250.0) {
            let preview = self.tune_preview_offset_hz.unwrap_or(0.0) as f32;
            let target = (peak - preview).clamp(-800.0, 800.0);
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

    fn continent_allowed(&self, spot: &Spot) -> bool {
        if !self.continent_filter {
            return true;
        }
        let Some(call) = &spot.callsign else {
            return true;
        };
        match self.resolver.continent_of(call) {
            Some(c) => self.show_continents[continent_index(c)],
            None => true,
        }
    }

    fn visible_spots(&self) -> Vec<Spot> {
        let mut spots: Vec<Spot> = self
            .skimmer_spots
            .iter()
            .filter(|s| s.snr_db >= self.min_spot_snr && self.continent_allowed(s))
            .cloned()
            .collect();
        match self.spot_sort {
            SpotSort::SnrDesc => spots.sort_by(|a, b| b.snr_db.total_cmp(&a.snr_db)),
            SpotSort::Frequency => spots.sort_by(|a, b| a.frequency_hz.total_cmp(&b.frequency_hz)),
            SpotSort::LastHeard => spots.sort_by_key(|s| s.last_heard),
            SpotSort::Callsign => spots.sort_by(|a, b| a.callsign.cmp(&b.callsign)),
        }
        spots
    }

    fn spot_labels(&self, center_hz: f64) -> Vec<SpotLabel> {
        self.visible_spots()
            .iter()
            .filter_map(|s| {
                let text = s.callsign.clone()?;
                Some(SpotLabel {
                    offset_hz: (s.frequency_hz - center_hz) as f32,
                    text,
                    cq: s.kind == SpotKind::CallingCq,
                })
            })
            .collect()
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
        let (zero, lock, notch, blank, nr, agc, narrow, widen, rit_dn, rit_up, full, mute) =
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
            self.cw.passband_hz = (self.cw.passband_hz - 25.0).clamp(CW_PASSBAND_MIN_HZ, CW_PASSBAND_MAX_HZ);
        }
        if widen {
            self.cw.passband_hz = (self.cw.passband_hz + 25.0).clamp(CW_PASSBAND_MIN_HZ, CW_PASSBAND_MAX_HZ);
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
    }

    fn update_texture(&mut self, ctx: &egui::Context) {
        let span = self.plot_view.view_span_hz(self.sample_rate);
        let pan = self.plot_view.pan_offset_hz;
        let view = extract_view_window(&self.latest, self.sample_rate, span, pan);
        let w = view.len().max(1);
        let h = WATERFALL_ROWS;
        let mut pixels = vec![Color32::BLACK; w * h];
        for (y, row) in self.rows.iter().enumerate() {
            let row_view = extract_view_window(row, self.sample_rate, span, pan);
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

    fn update_history_texture(&mut self, ctx: &egui::Context) {
        let rows = self.slow.rows();
        if rows.is_empty() {
            return;
        }
        let span = self.plot_view.view_span_hz(self.sample_rate);
        let pan = self.plot_view.pan_offset_hz;
        let w = extract_view_window(&self.latest, self.sample_rate, span, pan)
            .len()
            .max(1);
        let h = rows.len();
        let mut pixels = vec![Color32::BLACK; w * h];
        for (y, row) in rows.iter().enumerate() {
            let row_view = extract_view_window(row, self.sample_rate, span, pan);
            let base = y * w;
            for (x, &db) in row_view.iter().enumerate() {
                if x < w {
                    pixels[base + x] = db_to_colour(db, self.ref_db, self.range_db);
                }
            }
        }
        let image = egui::ColorImage::new([w, h], pixels);
        match &mut self.slow_texture {
            Some(tex) => tex.set(image, egui::TextureOptions::LINEAR),
            none => {
                *none =
                    Some(ctx.load_texture("slow_waterfall", image, egui::TextureOptions::NEAREST));
            }
        }
    }

    fn history_panel(&mut self, ui: &mut egui::Ui) {
        section_heading(ui, "Band history (last 10 min · peak-hold)");
        let span = self.plot_view.view_span_hz(self.sample_rate);
        let pan = self.plot_view.pan_offset_hz;
        let size = egui::vec2(ui.available_width(), ui.available_height().max(40.0));
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());

        if let Some(tex) = &self.slow_texture {
            ui.painter().image(
                tex.id(),
                rect,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        } else {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "collecting history…",
                egui::FontId::proportional(12.0),
                MUTED,
            );
        }

        let painter = ui.painter().clone();
        for ann in self.slow.annotations() {
            let x = crate::interaction::offset_hz_to_x(ann.offset_hz as f64, rect, span, pan);
            if x < rect.left() || x > rect.right() {
                continue;
            }
            let age = ann.at.elapsed().as_secs_f32();
            let y = (rect.top() + (age / 600.0) * rect.height()).clamp(rect.top(), rect.bottom());
            painter.line_segment(
                [egui::pos2(x, y - 4.0), egui::pos2(x, y + 4.0)],
                egui::Stroke::new(1.5, WARN),
            );
            painter.text(
                egui::pos2(x + 3.0, y),
                egui::Align2::LEFT_CENTER,
                &ann.label,
                egui::FontId::proportional(10.0),
                WARN,
            );
        }
    }

    fn connect_screen(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(60.0);
            ui.heading("hfsdr — connect a receiver");
            ui.add_space(12.0);
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_max_width(440.0);
                egui::Grid::new("connect_form")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Source");
                        ui.horizontal(|ui| {
                            ui.selectable_value(&mut self.form_kind, SourceKind::Kiwi, "KiwiSDR");
                            #[cfg(feature = "airspy")]
                            ui.selectable_value(&mut self.form_kind, SourceKind::Airspy, "Airspy");
                        });
                        ui.end_row();

                        if self.form_kind == SourceKind::Kiwi {
                            ui.label("Host");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.form_host)
                                    .hint_text("kiwi.example.com")
                                    .desired_width(240.0),
                            );
                            ui.end_row();

                            ui.label("Port");
                            ui.add(egui::DragValue::new(&mut self.form_port).range(1..=65535));
                            ui.end_row();
                        }

                        ui.label("Center");
                        ui.add(
                            egui::DragValue::new(&mut self.form_center_mhz)
                                .range(0.0..=60.0)
                                .speed(0.001)
                                .suffix(" MHz")
                                .fixed_decimals(3),
                        );
                        ui.end_row();
                    });

                ui.add_space(10.0);
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
                });

                ui.add_space(8.0);
                match &self.conn_state {
                    ConnState::Connecting { label } => {
                        ui.colored_label(WARN, format!("Connecting to {label}…"));
                    }
                    ConnState::Reconnecting { attempt, retry_in_s } => {
                        ui.colored_label(
                            WARN,
                            format!("Reconnecting (attempt {attempt}) in {retry_in_s:.0}s…"),
                        );
                    }
                    _ => {}
                }
                if let Some(err) = &self.last_error {
                    ui.colored_label(Color32::from_rgb(248, 113, 113), err);
                }
            });

            if !self.recent_hosts.is_empty() {
                ui.add_space(12.0);
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

            ui.add_space(16.0);
            section_hint(
                ui,
                "Tip: launch with `waterfall kiwi <host> [port] [center_hz]` to auto-connect.",
            );
        });
    }

    fn status_banner(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            match &self.conn_state {
                ConnState::Streaming if self.stats.slow => {
                    badge(ui, "SLOW LINK", WARN);
                }
                ConnState::Streaming => {
                    badge(ui, "STREAMING", OK);
                }
                ConnState::Reconnecting { attempt, retry_in_s } => {
                    badge(
                        ui,
                        &format!("RECONNECTING #{attempt} ({retry_in_s:.0}s)"),
                        WARN,
                    );
                }
                ConnState::Connecting { .. } => badge(ui, "CONNECTING", WARN),
                _ => badge(ui, "OFFLINE", MUTED),
            }
            ui.separator();
            ui.label(format!("{:.3} MHz", self.center_khz / 1000.0));
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
                if ui.button("Disconnect").clicked() {
                    self.engine.send(EngineCommand::Disconnect);
                    self.rows.clear();
                }
                ui.separator();
                ui.toggle_value(&mut self.show_right, "Right");
                ui.toggle_value(&mut self.show_history, "History");
                ui.toggle_value(&mut self.show_left, "Left");
                if self.stats.slow {
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
                self.connection_card(ui);
                self.receiver_card(ui);
                self.frequency_card(ui);
                self.skimmer_card(ui);
            });
    }

    fn right_panel(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                self.cw_demod_card(ui);
                self.filter_pipeline_card(ui);
                self.notch_card(ui);
                self.audio_card(ui);
                self.display_section(ui);
                self.performance_section(ui);

                ui.add_space(4.0);
                section_hint(
                    ui,
                    "Keys: Z zero-beat · L pitch-lock · N notch · B NB · R NR · A AGC · [ ] width · , . RIT · F full · Space mute",
                );
            });
    }

    fn connection_card(&mut self, ui: &mut egui::Ui) {
        section_card(ui, |ui| {
            section_heading(ui, "Connection");
            match &self.conn_state {
                ConnState::Streaming if self.stats.slow => badge(ui, "slow / unstable", WARN),
                ConnState::Streaming => badge(ui, "streaming", OK),
                ConnState::Connecting { .. } => badge(ui, "connecting", WARN),
                ConnState::Reconnecting { attempt, retry_in_s } => {
                    badge(ui, &format!("reconnect #{attempt} ({retry_in_s:.0}s)"), WARN)
                }
                ConnState::Disconnected => badge(ui, "offline", MUTED),
            }
            ui.add_space(4.0);
            stat_row(ui, "Source", if self.is_kiwi { "KiwiSDR" } else { "Airspy" });
            stat_row(ui, "Effective", format!("{:.1} kS/s", self.stats.effective_sps / 1000.0));
            stat_row(ui, "Dropped", self.stats.dropped.to_string());
            if let Some(rssi) = self.stats.rssi_dbm {
                stat_row(ui, "S-meter", format!("{rssi:.1} dBm"));
            }
            ui.add_space(4.0);
            if ui.button("Disconnect").clicked() {
                self.engine.send(EngineCommand::Disconnect);
                self.rows.clear();
            }
        });
    }

    fn display_section(&mut self, ui: &mut egui::Ui) {
        collapsible_section(ui, "display", "Display", false, |ui| {
            scroll_slider_f32(ui, &mut self.plot_view.zoom, 0.04..=1.0, "Zoom");
            if ui.small_button("Full span (F)").clicked() {
                self.plot_view.zoom = 1.0;
                self.plot_view.pan_offset_hz = 0.0;
            }
            scroll_slider_f32(ui, &mut self.ref_db, -120.0..=20.0, "Ref dB");
            scroll_slider_f32(ui, &mut self.range_db, 20.0..=140.0, "Range dB");
            scroll_slider_f32(ui, &mut self.smooth_alpha, 0.05..=0.45, "Smoothing");
        });
    }

    fn performance_section(&mut self, ui: &mut egui::Ui) {
        collapsible_section(ui, "perf", "Performance", false, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("FFT").small().color(MUTED));
                for n in [1024usize, 2048, 4096, 8192] {
                    if ui.selectable_label(self.fft_size == n, n.to_string()).clicked() {
                        self.fft_size = n;
                    }
                }
            });

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

    fn receiver_card(&mut self, ui: &mut egui::Ui) {
        section_card(ui, |ui| {
            section_heading(ui, "Receiver");
            stat_row(ui, "RX center", format!("{:.3} MHz", self.center_khz / 1000.0));
            stat_row(ui, "Listen", format!("{:.0} Hz", self.listen_offset_hz()));
            stat_row(ui, "SNR", format!("{:.0} dB", self.last_snr_db));
            stat_row(ui, "IQ", format!("{:.1} kS/s", self.sample_rate / 1000.0));
        });
    }

    fn frequency_card(&mut self, ui: &mut egui::Ui) {
        section_card(ui, |ui| {
            section_heading(ui, "Frequency");
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
                if ui.button("Zero-beat (Z)").clicked() {
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
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new("BW").small().color(MUTED));
                for (label, hz) in FILTER_PRESETS {
                    if ui.selectable_label(self.cw.passband_hz.round() == hz, label).clicked() {
                        self.cw.passband_hz = hz;
                    }
                }
            });
            scroll_slider_log_f32(
                ui,
                &mut self.cw.passband_hz,
                CW_PASSBAND_MIN_HZ..=CW_PASSBAND_MAX_HZ,
                "Audio BW",
            );
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Shape").small().color(MUTED));
                window_choice(ui, &mut self.cw.window, WindowKind::Gaussian, "Gauss");
                window_choice(ui, &mut self.cw.window, WindowKind::RaisedCosine, "RaisedCos");
                window_choice(ui, &mut self.cw.window, WindowKind::Blackman, "Blackman");
            });
            section_hint(ui, "Ctrl+scroll on plot: BW · drag cyan edges");
        });
    }

    fn filter_pipeline_card(&mut self, ui: &mut egui::Ui) {
        collapsible_section(ui, "pipeline", "Filter pipeline", true, |ui| {
            section_hint(ui, "Each stage toggles independently (stackable).");

            ui.checkbox(&mut self.cw.noise_blanker.enabled, "Noise blanker (B) — impulse QRN");
            if self.cw.noise_blanker.enabled {
                scroll_slider_f32(ui, &mut self.cw.noise_blanker.threshold, 2.0..=12.0, "NB threshold");
                let mut width = self.cw.noise_blanker.width as f32;
                scroll_slider_f32(ui, &mut width, 1.0..=30.0, "NB width");
                self.cw.noise_blanker.width = width.round() as usize;
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
                scroll_slider_f32(ui, &mut self.cw.noise_reduction.level, 0.0..=0.9, "NR level");
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

    fn skimmer_card(&mut self, ui: &mut egui::Ui) {
        section_card(ui, |ui| {
            section_heading(ui, "Skimmer");
            toggle(ui, &mut self.skimmer_enabled, "Decode whole span");
            toggle(ui, &mut self.show_history, "Band history");
            if self.skimmer_enabled {
                stat_row(ui, "Decoders", self.skimmer_channels.to_string());
                stat_row(ui, "Spots", self.skimmer_spots.len().to_string());
            }
        });

        if !self.skimmer_enabled {
            return;
        }

        collapsible_section(ui, "spots", "Spots", true, |ui| {
            scroll_slider_f32(ui, &mut self.min_spot_snr, 0.0..=40.0, "Min SNR");
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
            ui.separator();
            self.spot_table(ui);
        });
    }

    fn spot_table(&mut self, ui: &mut egui::Ui) {
        let spots = self.visible_spots();
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
            })
            .body(|mut body| {
                for spot in &spots {
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

    fn audio_card(&mut self, ui: &mut egui::Ui) {
        section_card(ui, |ui| {
            section_heading(ui, "Audio");
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
        });
    }

    fn central_panel(&mut self, ui: &mut egui::Ui) {
        self.plot_view.clamp_pan(self.sample_rate);
        let span = self.plot_view.view_span_hz(self.sample_rate);
        let pan = self.plot_view.pan_offset_hz;
        let trace = display_trace(
            &self.latest,
            &mut self.smoothed_trace,
            self.sample_rate,
            span,
            pan,
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

        let mut params = crate::widgets::PlotParams {
            sample_rate: self.sample_rate,
            passband_hz: self.cw.passband_hz,
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
            ui.allocate_space(egui::vec2(ui.available_width(), ui.available_height()));
            ui.centered_and_justified(|ui| {
                ui.label("Waiting for IQ data…");
            });
        }

        self.apply_plot_actions(plot_actions);
    }
}

fn window_choice(ui: &mut egui::Ui, current: &mut WindowKind, kind: WindowKind, label: &str) {
    if ui.selectable_label(*current == kind, label).clicked() {
        *current = kind;
    }
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

fn continent_index(c: Continent) -> usize {
    Continent::ALL.iter().position(|&x| x == c).unwrap_or(0)
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
            self.engine.send(EngineCommand::Connect(req));
        }

        self.handle_shortcuts(&ctx);
        self.pump_engine();

        let has_data = !self.rows.is_empty();
        let show_main = matches!(self.conn_state, ConnState::Streaming)
            || (has_data
                && matches!(
                    self.conn_state,
                    ConnState::Reconnecting { .. } | ConnState::Connecting { .. }
                ));

        if !show_main {
            egui::CentralPanel::default().show_inside(ui, |ui| self.connect_screen(ui));
            self.autosave();
            ctx.request_repaint_after(Duration::from_millis(100));
            return;
        }

        // Lazy texture rebuild: only when new rows arrived or the view changed.
        let span = self.plot_view.view_span_hz(self.sample_rate);
        let pan = self.plot_view.pan_offset_hz;
        let view_changed = span != self.last_tex_span || pan != self.last_tex_pan;
        if self.textures_dirty || view_changed {
            self.update_texture(&ctx);
            if self.show_history {
                self.update_history_texture(&ctx);
            }
            self.textures_dirty = false;
            self.last_tex_span = span;
            self.last_tex_pan = pan;
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

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show_inside(ui, |ui| {
                self.central_panel(ui);
            });

        self.apply_radio_settings();
        self.autosave();

        let frame_ms = (1000 / self.target_fps.max(1)).max(8) as u64;
        ctx.request_repaint_after(Duration::from_millis(frame_ms));
    }

    fn on_exit(&mut self) {
        self.current_settings().save();
    }
}
