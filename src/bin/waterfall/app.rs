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
    decimation_factor, compose_panadapter_row, panadapter_output_bins, stretch_row_to_width,
    strongest_offset_hz, Continent,
    ContinentResolver, AgcMode, ChannelFilterKind, ChannelOffsetHz, CwChannelSettings, RowFold, SlowWaterfall, SpectrumViewMapping, Spot,
    SpotKind, SpotSort, SkimmerConfig, SkimmerDecoderKind, channel_group_delay_ms, WindowKind,
    MAX_NOTCHES,
};
use hfsdr::kiwi::protocol::{man_gain_db_below_max, man_gain_from_db_below_max};

use crate::af_scope::{
    self, AfScopeParams, DualAgcParams, classify_level, rf_level_dbm, show_dual_agc_loop,
    show_status_rf_meter,
};
use crate::audio::AudioOutput;
use crate::colormap::db_to_colour;
use crate::controls::{
    preset_combo_f64, preset_combo_u32, scroll_slider_f32, scroll_slider_f32_step,
    scroll_slider_log_f32, vfo_wheel_khz,
};
use crate::display_levels::{
    display_levels_initialized_after_settings_load, estimate_levels, estimate_levels_from_rows,
    lock_display_levels_for_rf_tuning, should_auto_adjust_display_levels,
};
use crate::engine::{
    ConnState, EngineCommand, EngineHandle, EngineParams, EngineStats, FFT_SIZE, WATERFALL_ROWS,
};
use crate::ham_bands;
use crate::rf_view;
use crate::popup::{
    alert_banner, chip_row, configure_popup_window, ghost_button, inline_stats, list_row,
    popup_header, popup_scroll_body, popup_section, primary_button, secondary_button,
    segment_choice, PopupHeader,
};
use crate::iq_panel::{IqPanel, IqPanelCmd, IqPanelView};
use crate::interaction::{
    PlotAction, PlotFreqMapping, PlotInteraction, PlotViewState, RIT_MAX_HZ, RIT_MIN_HZ, NOTCH_WIDTH_MAX_HZ,
    NOTCH_WIDTH_MIN_HZ, suggest_notch_offset_hz,
};
use crate::interaction::{CW_PASSBAND_MAX_HZ, CW_PASSBAND_MIN_HZ, CW_PASSBAND_NARROW_MAX_HZ};
use crate::kiwi_directory::{GeoLocation, KiwiReceiver};
use crate::log;
use crate::pipeline_flow::{PipelineFlow, PipelineSnapshot, PipelineStage};
use crate::settings::{AppSettings, NotchData};
use crate::source::{AirspySettings, ConnectRequest, KiwiSettings, QmxSettings, RtlSdrSettings, SourceKind};
use crate::spot_filter::{
    build_spot_labels, continent_index, filter_spots, SpotFilterConfig, SpotLabelConfig,
};
use crate::theme::{
    apply, attach_rich_tooltip, band_lock_toggle, clickable_badge, collapsible_section,
    panel_toggle, section_card, section_frame, section_heading, section_heading_with_tip, section_hint,
    side_panel_frame, stat_row, stage_toggle, status_panel_frame, toggle, ACCENT, MUTED, OK, WARN,
};
use crate::widgets::{
    update_trace, PanadapterPlot, SpotLabel, TraceViewKey, SCOPE_HEIGHT,
};

const SMOOTH_ALPHA: f32 = 0.09;

/// Minimum RX panel width (VFO digit wheels + section margins).
const LEFT_PANEL_MIN_W: f32 = 288.0;
const LEFT_PANEL_MAX_W: f32 = 440.0;
/// Minimum DSP panel width (AF scope, stage toggles, labeled sliders).
const RIGHT_PANEL_MIN_W: f32 = 312.0;
const RIGHT_PANEL_MAX_W: f32 = 480.0;

/// CW band plan: calling frequency + typical CW segment width for panadapter zoom.
struct CwBandPreset {
    label: &'static str,
    center_hz: f64,
    segment_hz: f32,
}

const CW_HF_BAND_PRESETS: [CwBandPreset; 10] = [
    CwBandPreset { label: "160m", center_hz: 1_810_000.0, segment_hz: 30_000.0 },
    CwBandPreset { label: "80m", center_hz: 3_510_000.0, segment_hz: 80_000.0 },
    CwBandPreset { label: "60m", center_hz: 5_354_000.0, segment_hz: 56_000.0 },
    CwBandPreset { label: "40m", center_hz: 7_010_000.0, segment_hz: 40_000.0 },
    CwBandPreset { label: "30m", center_hz: 10_110_000.0, segment_hz: 40_000.0 },
    CwBandPreset { label: "20m", center_hz: 14_010_000.0, segment_hz: 70_000.0 },
    CwBandPreset { label: "17m", center_hz: 18_080_000.0, segment_hz: 43_000.0 },
    CwBandPreset { label: "15m", center_hz: 21_010_000.0, segment_hz: 70_000.0 },
    CwBandPreset { label: "12m", center_hz: 24_900_000.0, segment_hz: 40_000.0 },
    CwBandPreset { label: "10m", center_hz: 28_010_000.0, segment_hz: 70_000.0 },
];

/// VHF and up — separate from HF so the band grid matches the band plan.
const CW_VHF_BAND_PRESETS: [CwBandPreset; 1] = [
    CwBandPreset { label: "6m", center_hz: 50_090_000.0, segment_hz: 100_000.0 },
];

const DEFAULT_CENTER_HZ: f64 = 14_010_000.0;

const BFO_PRESETS: [(&str, f32); 4] = [("500", 500.0), ("600", 600.0), ("700", 700.0), ("800", 800.0)];
const FILTER_PRESETS: [(&str, f32); 6] = [
    ("50", 50.0),
    ("100", 100.0),
    ("250", 250.0),
    ("500", 500.0),
    ("1k", 1_000.0),
    ("2k", 2_000.0),
];

const KIWI_IQ_RATE_PRESETS: &[(&str, u32)] = &[
    ("12 kHz (default)", 12_000),
    ("20.25 kHz (3-ch)", 20_250),
];

const KIWI_BW_PRESETS: &[(&str, u32)] = &[
    ("Full (max)", 0),
    ("±5 kHz", 5_000),
    ("±3 kHz", 3_000),
    ("±2.5 kHz", 2_500),
];

const KIWI_RESAMPLE_PRESETS: &[(&str, u32)] = &[
    ("None (native)", 0),
    ("12 kHz", 12_000),
    ("8 kHz", 8_000),
    ("6 kHz", 6_000),
    ("4.8 kHz", 4_800),
];

const KIWI_LO_PRESETS: &[(&str, f64)] = &[
    ("None", 0.0),
    ("9.75 MHz", 9_750.0),
    ("10.0 MHz", 10_000.0),
    ("10.45 MHz", 10_450.0),
    ("144 MHz", 144_000.0),
];

const KIWI_AR_OUT_PRESETS: &[(&str, u32)] = &[
    ("44.1 kHz", 44_100),
    ("48 kHz", 48_000),
    ("96 kHz", 96_000),
];

#[cfg(feature = "airspy")]
const AIRSPY_SAMPLE_RATE_PRESETS: &[(&str, u32)] = &[
    ("384 kHz (recommended)", 384_000),
    ("768 kHz", 768_000),
    ("192 kHz", 192_000),
    ("96 kHz", 96_000),
    ("48 kHz", 48_000),
    ("24 kHz", 24_000),
    ("12 kHz", 12_000),
];

#[cfg(feature = "rtlsdr")]
const RTLSDR_SAMPLE_RATE_PRESETS: &[(&str, u32)] = &[
    ("2.048 MHz (recommended)", 2_048_000),
    ("2.4 MHz", 2_400_000),
    ("1.92 MHz", 1_920_000),
    ("1.024 MHz", 1_024_000),
    ("960 kHz", 960_000),
    ("320 kHz", 320_000),
    ("250 kHz", 250_000),
];

#[cfg(feature = "rtlsdr")]
const RTLSDR_PROCESS_RATE_PRESETS: &[(&str, u32)] = &[
    ("Native (full rate)", 0),
    ("96 kHz", 96_000),
    ("48 kHz", 48_000),
    ("24 kHz", 24_000),
    ("12 kHz", 12_000),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct StorageKey {
    tex_width: u32,
    storage_span_hz: u32,
    row_rate_hz: u32,
}

impl StorageKey {
    fn from(storage: &SpectrumViewMapping, tex_width: usize) -> Self {
        Self {
            tex_width: tex_width as u32,
            storage_span_hz: storage.view_span_hz.round() as u32,
            row_rate_hz: storage.row_rate_hz.round() as u32,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ViewportKey {
    view_span_hz: u32,
    pan_bits: u64,
    plot_width: u32,
}

impl ViewportKey {
    fn from_view(view_span_hz: f32, pan_offset_hz: f64, plot_width: usize) -> Self {
        Self {
            view_span_hz: view_span_hz.round() as u32,
            pan_bits: pan_offset_hz.to_bits(),
            plot_width: plot_width as u32,
        }
    }
}

#[cfg(feature = "airspy")]
const AIRSPY_PROCESS_RATE_PRESETS: &[(&str, u32)] = &[
    ("48 kHz (recommended)", 48_000),
    ("Native (full bandwidth)", 0),
    ("96 kHz", 96_000),
    ("192 kHz", 192_000),
];

#[cfg(feature = "qmx")]
const QMX_PROCESS_RATE_PRESETS: &[(&str, u32)] = &[
    ("24 kHz (recommended)", 24_000),
    ("Native (48 kHz)", 0),
    ("12 kHz", 12_000),
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
    form_kiwi: KiwiSettings,
    form_sample_rate: u32,
    form_airspy: AirspySettings,
    last_airspy_rf: AirspySettings,
    form_rtlsdr: RtlSdrSettings,
    last_rtlsdr_rf: RtlSdrSettings,
    form_qmx: QmxSettings,
    last_qmx_rf: QmxSettings,

    sample_rate: f32,
    center_khz: f64,
    last_center_khz: f64,
    is_kiwi: bool,

    /// The toggleable CW listen-chain configuration (owned by the UI).
    cw: CwChannelSettings,
    rit_hz: f32,
    pitch_lock: bool,
    lock_ham_bands: bool,
    agc_rf_on: bool,
    last_agc_rf_on: bool,
    last_kiwi_man_gain: u8,
    last_kiwi_rf_attn_db: f32,
    last_kiwi_has_rf_attn: bool,
    last_snr_db: f32,

    rows: VecDeque<Vec<f32>>,
    latest: Vec<f32>,
    smoothed_trace: Vec<f32>,
    trace_composed: Vec<f32>,
    trace_view_key: TraceViewKey,
    overview_smoothed: Vec<f32>,
    overview_composed: Vec<f32>,
    overview_view_key: TraceViewKey,
    latest_frame_tick: bool,
    waterfall_storage_pixels: Vec<Color32>,
    storage_tex_width: usize,
    last_storage_key: Option<StorageKey>,
    waterfall_viewport_texture: Option<egui::TextureHandle>,
    waterfall_viewport_pixels: Vec<Color32>,
    viewport_tex_width: usize,
    last_viewport_key: Option<ViewportKey>,
    textures_dirty: bool,
    force_texture_full: bool,
    pending_row_appends: usize,
    pending_viewport_row_appends: usize,
    last_display_levels_at: Option<Instant>,
    waterfall_row_scratch: Vec<f32>,

    ref_db: f32,
    range_db: f32,
    display_levels_initialized: bool,
    display_auto_track: bool,
    show_band_overview: bool,
    pan_step_hz: f32,
    pan_step_fast_hz: f32,
    arrow_hold: Option<(egui::Key, Instant)>,
    smooth_alpha: f32,
    waterfall_avg: u8,
    waterfall_rows: usize,
    target_fps: u32,
    fft_size: usize,
    fft_auto: bool,
    full_drain_spectrum: bool,

    audio_devices: Vec<String>,
    selected_audio_device: usize,
    last_audio_device: usize,
    audio_enabled: bool,
    volume: f32,
    audio_scope: Vec<f32>,

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
    show_shortcuts: bool,
    /// AF tuning scope in CW demod panel (toggle with G or status bar).
    show_af_scope: bool,
    /// S-meter + dual AGC bars (status bar Meter toggle).
    show_smeter: bool,
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
    show_iq_drawer: bool,
    show_pipeline_drawer: bool,
    pipeline_flow: PipelineFlow,
    /// Saved manual-notch `enabled` flags while bypassed from the pipeline diagram.
    notch_bypass_stash: Option<[bool; MAX_NOTCHES]>,
    iq: IqPanel,

    last_settings_snapshot: Option<AppSettings>,
    settings_dirty_at: Option<std::time::Instant>,

    panadapter_plot: PanadapterPlot,
    plot_view: PlotViewState,
    plot_interaction: PlotInteraction,
    hover_offset_hz: Option<f64>,
    last_plot_interaction_rect: Option<egui::Rect>,
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
            form_kiwi: KiwiSettings::default(),
            form_sample_rate: 384_000,
            form_airspy: AirspySettings::default(),
            last_airspy_rf: AirspySettings::default(),
            form_rtlsdr: RtlSdrSettings::default(),
            last_rtlsdr_rf: RtlSdrSettings::default(),
            form_qmx: QmxSettings::default(),
            last_qmx_rf: QmxSettings::default(),
            sample_rate: 12_000.0,
            center_khz: DEFAULT_CENTER_HZ / 1000.0,
            last_center_khz: DEFAULT_CENTER_HZ / 1000.0,
            is_kiwi: false,
            cw: CwChannelSettings::default(),
            rit_hz: 0.0,
            pitch_lock: false,
            lock_ham_bands: true,
            agc_rf_on: true,
            last_agc_rf_on: true,
            last_kiwi_man_gain: hfsdr::kiwi::protocol::KIWI_MAN_GAIN_DEFAULT,
            last_kiwi_rf_attn_db: 0.0,
            last_kiwi_has_rf_attn: false,
            last_snr_db: 0.0,
            rows: VecDeque::with_capacity(WATERFALL_ROWS),
            latest: vec![-120.0; FFT_SIZE],
            smoothed_trace: Vec::new(),
            trace_composed: Vec::new(),
            trace_view_key: TraceViewKey::new(0.0, 0.0, 0.0, 0.0, 0),
            overview_smoothed: Vec::new(),
            overview_composed: Vec::new(),
            overview_view_key: TraceViewKey::new(0.0, 0.0, 0.0, 0.0, 0),
            latest_frame_tick: false,
            waterfall_storage_pixels: Vec::new(),
            storage_tex_width: 0,
            last_storage_key: None,
            waterfall_viewport_texture: None,
            waterfall_viewport_pixels: Vec::new(),
            viewport_tex_width: 0,
            last_viewport_key: None,
            textures_dirty: false,
            force_texture_full: true,
            pending_row_appends: 0,
            pending_viewport_row_appends: 0,
            last_display_levels_at: None,
            waterfall_row_scratch: Vec::new(),

            ref_db: -65.0,
            range_db: crate::display_levels::DEFAULT_RANGE_DB,
            display_levels_initialized: false,
            display_auto_track: false,
            show_band_overview: false,
            pan_step_hz: 500.0,
            pan_step_fast_hz: 5000.0,
            arrow_hold: None,
            smooth_alpha: SMOOTH_ALPHA,
            waterfall_avg: 1,
            waterfall_rows: 0,
            target_fps: 30,
            fft_size: FFT_SIZE,
            fft_auto: true,
            full_drain_spectrum: false,
            audio_devices,
            selected_audio_device: 0,
            last_audio_device: 0,
            audio_enabled: true,
            volume: 1.0,
            audio_scope: Vec::new(),
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
            spot_max_age_secs: 180.0,
            spot_callsign_filter: String::new(),
            spot_label_limit: 40,
            scp_notice: None,
            scp_download_rx: None,
            scp_reload_pending: false,
            scp_reload_deadline: None,
            last_scp_loaded: false,
            filter_wide: false,
            show_console: false,
            show_shortcuts: false,
            show_af_scope: true,
            show_smeter: true,
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
            show_iq_drawer: false,
            show_pipeline_drawer: false,
            pipeline_flow: PipelineFlow::new(),
            notch_bypass_stash: None,
            iq: IqPanel::new(hfsdr::default_capture_dir()),

            last_settings_snapshot: None,
            settings_dirty_at: None,
            panadapter_plot: PanadapterPlot::new(),
            plot_view: PlotViewState::new(),
            plot_interaction: PlotInteraction::new(),
            hover_offset_hz: None,
            last_plot_interaction_rect: None,
            tune_preview_offset_hz: None,
            themed: false,
        };

        app.apply_settings(&saved);

        // Seed host/port from the most-recent connection; keep the tune point from settings/defaults.
        if let Some(r) = app.recent_hosts.first().cloned() {
            app.apply_connect_form(&r);
        }

        // CLI args take precedence and trigger an auto-connect on first frame.
        if let Some(req) = autoconnect {
            app.form_kind = req.kind;
            app.form_host = req.host.clone();
            app.form_port = req.port;
            app.form_kiwi = req.kiwi.clone();
            if req.sample_rate != 0 {
                app.form_sample_rate = req.sample_rate;
            }
            app.form_airspy = req.airspy.clone();
            app.form_rtlsdr = req.rtlsdr.clone();
            app.form_qmx = req.qmx.clone();
            app.center_khz = req.center_hz / 1000.0;
            app.clamp_center_to_ham_bands();
            app.last_center_khz = app.center_khz;
            app.pending_connect = Some(req);
            app.show_connection_drawer = false;
        }

        app.last_settings_snapshot = Some(app.current_settings());
        if let Some((geo, receivers)) = crate::kiwi_directory::load_cached_receivers() {
            app.kiwi_geo = geo;
            app.kiwi_nearby = receivers;
        }
        app.start_kiwi_directory_fetch(false);
        app.apply_default_view_zoom();
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

    fn apply_plot_actions(&mut self, actions: Vec<PlotAction>) {
        let iq_playback = self.stats.iq_playback;
        for action in actions {
            match action {
                PlotAction::TuneDeltaHz(delta) => {
                    if iq_playback {
                        self.plot_view.pan_offset_hz += delta;
                        self.plot_view.clamp_pan(
                            self.plot_full_span_hz(),
                            self.plot_max_zoom_out(),
                        );
                    } else {
                        self.invalidate_waterfall_history();
                        self.center_khz += delta / 1000.0;
                    }
                }
                PlotAction::CenterOnOffsetHz(offset) => {
                    if iq_playback {
                        self.rit_hz = (offset as f32).clamp(RIT_MIN_HZ, RIT_MAX_HZ);
                        self.tune_preview_offset_hz = None;
                    } else {
                        self.invalidate_waterfall_history();
                        self.center_khz += offset / 1000.0;
                        self.plot_view.pan_offset_hz = 0.0;
                        self.tune_preview_offset_hz = None;
                        self.clear_rit();
                    }
                }
                PlotAction::SetTunePreviewOffsetHz(offset) => {
                    self.tune_preview_offset_hz = Some(offset);
                }
                PlotAction::CommitTunePreview => {
                    if let Some(offset) = self.tune_preview_offset_hz {
                        if iq_playback {
                            self.rit_hz = (self.rit_hz as f64 + offset)
                                .clamp(RIT_MIN_HZ as f64, RIT_MAX_HZ as f64)
                                as f32;
                        } else {
                            self.invalidate_waterfall_history();
                            self.center_khz += offset / 1000.0;
                            self.plot_view.pan_offset_hz = 0.0;
                            self.clear_rit();
                        }
                    }
                    self.tune_preview_offset_hz = None;
                }
                PlotAction::ClearTunePreview => {
                    self.tune_preview_offset_hz = None;
                }
                PlotAction::PanViewDeltaHz(delta) => {
                    self.plot_view.pan_offset_hz += delta;
                    self.plot_view.clamp_pan(
                        self.plot_full_span_hz(),
                        self.plot_max_zoom_out(),
                    );
                }
                PlotAction::ZoomView(factor) => {
                    self.plot_view.zoom_by(
                        factor,
                        self.plot_full_span_hz(),
                        self.plot_max_zoom_out(),
                    );
                }
                PlotAction::SetPassbandHz(bw) => {
                    self.cw.passband_hz =
                        bw.clamp(CW_PASSBAND_MIN_HZ, self.passband_max_hz());
                }
                PlotAction::SetRitHz(rit) => {
                    self.rit_hz = rit.clamp(RIT_MIN_HZ, RIT_MAX_HZ);
                }
                PlotAction::SetViewPanHz(pan) => {
                    self.plot_view.pan_offset_hz = pan;
                    self.plot_view.clamp_pan(
                        self.plot_full_span_hz(),
                        self.plot_max_zoom_out(),
                    );
                }
                PlotAction::SetNotchOffset { slot, offset_hz } => {
                    if let Some(n) = self.cw.notches.get_mut(slot) {
                        n.offset_hz = offset_hz;
                    }
                }
                PlotAction::SetNotchWidth { slot, width_hz } => {
                    if let Some(n) = self.cw.notches.get_mut(slot) {
                        n.width_hz = width_hz.clamp(NOTCH_WIDTH_MIN_HZ, NOTCH_WIDTH_MAX_HZ);
                    }
                }
            }
        }
        self.clamp_center_to_ham_bands();
    }

    /// Keep RX center inside amateur band allocations when band lock is enabled.
    fn clamp_center_to_ham_bands(&mut self) {
        if !self.lock_ham_bands {
            return;
        }
        let clamped_khz = ham_bands::clamp_hz(self.center_khz * 1000.0) / 1000.0;
        if (clamped_khz - self.center_khz).abs() > f64::EPSILON {
            self.invalidate_waterfall_history();
            self.center_khz = clamped_khz;
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

    fn lock_display_levels_for_rf_tuning(&mut self) {
        lock_display_levels_for_rf_tuning(
            &mut self.display_auto_track,
            &mut self.display_levels_initialized,
        );
    }

    fn update_display_levels(&mut self) {
        if !should_auto_adjust_display_levels(
            self.display_levels_initialized,
            self.display_auto_track,
        ) {
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
        let ref_delta = (self.ref_db - ref_db).abs();
        let range_delta = (self.range_db - range_db).abs();
        if !self.display_levels_initialized || ref_delta > 0.35 || range_delta > 0.75 {
            self.ref_db = ref_db;
            self.range_db = range_db;
            self.force_texture_full = true;
            self.textures_dirty = true;
            self.display_levels_initialized = true;
        }
    }

    fn estimate_display_levels(&self) -> Option<(f32, f32)> {
        const ROWS_FOR_ESTIMATE: usize = 24;
        let view = self.spectrum_view();
        let compose = |row: &[f32]| {
            compose_panadapter_row(
                row,
                view.row_rate_hz,
                view.view_span_hz,
                view.data_span_hz,
                view.compose_pan_offset_hz,
                view.allow_band_padding,
            )
        };
        if self.rows.len() >= 8 {
            let n = self.rows.len().min(ROWS_FOR_ESTIMATE);
            let composed: Vec<Vec<f32>> = self
                .rows
                .iter()
                .take(n)
                .map(|row| compose(row))
                .collect();
            let refs: Vec<&[f32]> = composed.iter().map(Vec::as_slice).collect();
            estimate_levels_from_rows(&refs).or_else(|| estimate_levels(&compose(&self.latest)))
        } else {
            estimate_levels(&compose(&self.latest))
        }
    }

    fn iq_passband_hz(&self) -> f32 {
        rf_view::iq_passband_hz(
            self.is_kiwi,
            self.stats.iq_passband_hz,
            self.sample_rate,
        )
    }

    /// Span of the spectrum FFT chain — base for zoom, pan, clicks, and waterfall storage.
    fn plot_full_span_hz(&self) -> f32 {
        rf_view::spectrum_plot_span_hz(self.stats.spectrum_rate, self.iq_passband_hz())
    }

    fn plot_max_zoom_out(&self) -> f32 {
        rf_view::max_zoom_out(
            self.is_kiwi,
            self.iq_passband_hz(),
            self.band_overview_span_hz(),
        )
    }

    fn spectrum_view(&self) -> SpectrumViewMapping {
        rf_view::build_spectrum_view(
            self.is_kiwi,
            self.iq_passband_hz(),
            self.plot_full_span_hz(),
            self.band_overview_span_hz(),
            self.stats.spectrum_rate,
            self.stats.spectrum_zoomed,
            &self.plot_view,
        )
    }

    fn waterfall_storage_view(&self) -> SpectrumViewMapping {
        rf_view::build_waterfall_storage_view(
            self.is_kiwi,
            self.iq_passband_hz(),
            self.plot_full_span_hz(),
            self.band_overview_span_hz(),
            self.stats.spectrum_rate,
        )
    }

    fn storage_row_width(&self, storage: &SpectrumViewMapping, row_len: usize) -> usize {
        panadapter_output_bins(row_len, storage.view_span_hz, storage.data_span_hz).max(1)
    }

    /// Snap tuning so the strongest signal near the cursor lands at the BFO pitch.
    fn clear_rit(&mut self) {
        self.rit_hz = 0.0;
        if self.pitch_lock {
            self.pitch_lock = false;
        }
    }

    /// Snap carrier to the strongest signal in view and clear listen offset.
    fn zero_beat(&mut self) {
        let listen = self.listen_offset_hz() as f32;
        let view = self.spectrum_view();
        if let Some(peak) = strongest_offset_hz(&self.latest, view.row_rate_hz, listen, 400.0) {
            self.center_khz += (peak - listen) as f64 / 1000.0;
            self.clamp_center_to_ham_bands();
            self.invalidate_waterfall_history();
            self.clear_rit();
            self.tune_preview_offset_hz = None;
        }
    }

    /// Continuously steer RIT so a drifting signal keeps a constant audio pitch.
    fn apply_pitch_lock(&mut self) {
        if !self.pitch_lock {
            return;
        }
        let listen = self.listen_offset_hz() as f32;
        let view = self.spectrum_view();
        if let Some(peak) = strongest_offset_hz(&self.latest, view.row_rate_hz, listen, 250.0) {
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

    fn center_hz(&self) -> f64 {
        self.center_khz * 1000.0
    }

    fn update_plot_hover(&mut self, ctx: &egui::Context) {
        let Some(rect) = self.last_plot_interaction_rect else {
            self.hover_offset_hz = None;
            return;
        };
        let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) else {
            self.hover_offset_hz = None;
            return;
        };
        if !rect.contains(pos) {
            self.hover_offset_hz = None;
            return;
        }
        self.plot_view
            .clamp_pan(self.plot_full_span_hz(), self.plot_max_zoom_out());
        let view = self.spectrum_view();
        self.hover_offset_hz = Some(crate::interaction::x_to_offset_hz(
            pos.x,
            rect,
            view.view_span_hz,
            view.pan_offset_hz,
        ));
    }

    fn toggle_pipeline_stage(&mut self, stage: PipelineStage) {
        match stage {
            PipelineStage::NoiseBlanker => {
                self.cw.noise_blanker.enabled = !self.cw.noise_blanker.enabled;
            }
            PipelineStage::ManualNotches => self.toggle_notch_bypass(),
            PipelineStage::ListenNco => {
                self.cw.diagnostic.listen_nco = !self.cw.diagnostic.listen_nco;
            }
            PipelineStage::DecimatorFir => {
                self.cw.diagnostic.decim_fir = !self.cw.diagnostic.decim_fir;
            }
            PipelineStage::ChannelFir => {
                self.cw.diagnostic.channel_fir = !self.cw.diagnostic.channel_fir;
            }
            PipelineStage::Bfo => {
                self.cw.diagnostic.bfo = !self.cw.diagnostic.bfo;
            }
            PipelineStage::Agc => self.cw.agc.enabled = !self.cw.agc.enabled,
            PipelineStage::Apf => self.cw.apf.enabled = !self.cw.apf.enabled,
            PipelineStage::AutoNotch => self.cw.auto_notch.enabled = !self.cw.auto_notch.enabled,
            PipelineStage::NoiseReduction => {
                self.cw.noise_reduction.enabled = !self.cw.noise_reduction.enabled;
            }
            PipelineStage::Skimmer => self.skimmer_enabled = !self.skimmer_enabled,
            PipelineStage::AudioOutput => self.audio_enabled = !self.audio_enabled,
        }
        let on = match stage {
            PipelineStage::NoiseBlanker => self.cw.noise_blanker.enabled,
            PipelineStage::ManualNotches => self.cw.notches.iter().any(|n| n.enabled),
            PipelineStage::ListenNco => !self.cw.diagnostic.listen_nco,
            PipelineStage::DecimatorFir => !self.cw.diagnostic.decim_fir,
            PipelineStage::ChannelFir => !self.cw.diagnostic.channel_fir,
            PipelineStage::Bfo => !self.cw.diagnostic.bfo,
            PipelineStage::Agc => self.cw.agc.enabled,
            PipelineStage::Apf => self.cw.apf.enabled,
            PipelineStage::AutoNotch => self.cw.auto_notch.enabled,
            PipelineStage::NoiseReduction => self.cw.noise_reduction.enabled,
            PipelineStage::Skimmer => self.skimmer_enabled,
            PipelineStage::AudioOutput => self.audio_enabled,
        };
        let tag = if stage.is_diagnostic() { "diag" } else { "pipeline" };
        log::info(&format!(
            "{tag} {} {}",
            stage.label(),
            if on { "on" } else { "bypassed" }
        ));
        if !stage.is_diagnostic() {
            self.settings_dirty_at = Some(Instant::now());
        }
    }

    fn toggle_notch_bypass(&mut self) {
        let any = self.cw.notches.iter().any(|n| n.enabled);
        if any {
            let mut stash = [false; MAX_NOTCHES];
            for (slot, n) in self.cw.notches.iter_mut().enumerate() {
                stash[slot] = n.enabled;
                n.enabled = false;
            }
            self.notch_bypass_stash = Some(stash);
            return;
        }
        if let Some(stash) = self.notch_bypass_stash.take() {
            for (n, was) in self.cw.notches.iter_mut().zip(stash.iter()) {
                n.enabled = *was;
            }
        }
    }

    fn arm_manual_notch(&mut self, slot: usize, offset_hz: Option<ChannelOffsetHz>) {
        let listen = ChannelOffsetHz::new(self.listen_offset_hz() as f32);
        let other: Vec<ChannelOffsetHz> = self
            .cw
            .notches
            .iter()
            .enumerate()
            .filter(|(i, n)| *i != slot && n.enabled)
            .map(|(_, n)| n.offset_hz)
            .collect();
        let offset = offset_hz.unwrap_or_else(|| suggest_notch_offset_hz(listen, &other));
        let Some(notch) = self.cw.notches.get_mut(slot) else {
            return;
        };
        notch.enabled = true;
        notch.offset_hz = offset;
        if notch.width_hz < NOTCH_WIDTH_MIN_HZ {
            notch.width_hz = 50.0;
        }
        self.notch_bypass_stash = None;
    }

    fn enabled_notches(&self) -> Vec<crate::interaction::NotchMarker> {
        self.cw
            .notches
            .iter()
            .enumerate()
            .filter(|(_, n)| n.enabled)
            .map(|(slot, n)| crate::interaction::NotchMarker {
                slot,
                offset_hz: n.offset_hz,
                width_hz: n.width_hz,
            })
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
        collapsible_section(ui, "scp", "MASTER.SCP", None, false, |ui| {
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

    fn cw_band_for_center(center_hz: f64) -> Option<&'static CwBandPreset> {
        CW_HF_BAND_PRESETS
            .iter()
            .chain(CW_VHF_BAND_PRESETS.iter())
            .find(|band| (center_hz - band.center_hz).abs() < 25_000.0)
    }

    fn band_preset_buttons(&mut self, ui: &mut egui::Ui, bands: &[CwBandPreset]) {
        ui.horizontal_wrapped(|ui| {
            for band in bands {
                let selected = (self.center_khz * 1000.0).round() == band.center_hz;
                if ui.selectable_label(selected, band.label).clicked() {
                    self.select_cw_band(band);
                }
            }
        });
    }

    fn band_overview_span_hz(&self) -> f32 {
        let iq = self.plot_full_span_hz();
        let center = self.center_khz * 1000.0;
        Self::cw_band_for_center(center)
            .map(|band| band.segment_hz.max(iq))
            .unwrap_or(iq)
    }

    /// Default panadapter span: CW segment for the current band (wider than IQ on Kiwi).
    fn default_cw_segment_hz(&self) -> f32 {
        let center = self.center_khz * 1000.0;
        Self::cw_band_for_center(center)
            .map(|band| band.segment_hz)
            .unwrap_or(self.band_overview_span_hz())
    }

    fn apply_default_view_zoom(&mut self) {
        self.plot_view.zoom_to_full_span();
        self.plot_view
            .clamp_pan(self.plot_full_span_hz(), self.plot_max_zoom_out());
    }

    fn select_cw_band(&mut self, band: &CwBandPreset) {
        self.center_khz = band.center_hz / 1000.0;
        self.plot_view.pan_offset_hz = 0.0;
        self.tune_preview_offset_hz = None;
        self.clear_rit();
        self.invalidate_waterfall_history();
        self.apply_radio_settings();
    }

    fn tune_to_hz(&mut self, frequency_hz: f64) {
        if (frequency_hz / 1000.0 - self.center_khz).abs() > f64::EPSILON {
            self.invalidate_waterfall_history();
        }
        self.center_khz = frequency_hz / 1000.0;
        self.clamp_center_to_ham_bands();
        self.plot_view.pan_offset_hz = 0.0;
        self.tune_preview_offset_hz = None;
        self.clear_rit();
    }

    fn kiwi_rf_live(&self) -> bool {
        self.form_kind == SourceKind::Kiwi && matches!(self.conn_state, ConnState::Streaming)
    }

    fn sync_kiwi_rf_now(&mut self) {
        if !self.kiwi_rf_live() {
            return;
        }
        let mut rf_changed = false;
        if self.agc_rf_on != self.last_agc_rf_on {
            self.engine.send(EngineCommand::SetRfAgc(self.agc_rf_on));
            self.last_agc_rf_on = self.agc_rf_on;
            self.form_kiwi.rf_agc_on = self.agc_rf_on;
            rf_changed = true;
        }
        if self.form_kiwi.man_gain != self.last_kiwi_man_gain {
            self.engine
                .send(EngineCommand::SetKiwiManGain(self.form_kiwi.man_gain));
            self.last_kiwi_man_gain = self.form_kiwi.man_gain;
            rf_changed = true;
        }
        if rf_changed {
            self.lock_display_levels_for_rf_tuning();
        }
    }

    fn rf_meter_dbm(&self) -> f32 {
        rf_level_dbm(self.stats.rssi_dbm, self.stats.iq_rf_level)
    }

    fn apply_radio_settings(&mut self) {
        if (self.center_khz - self.last_center_khz).abs() > f64::EPSILON {
            self.invalidate_waterfall_history();
            self.engine.send(EngineCommand::Tune(self.center_khz * 1000.0));
            self.last_center_khz = self.center_khz;
        }
        self.sync_kiwi_rf_now();
        self.apply_kiwi_rf_attn_settings();
        self.apply_airspy_live_settings();
        self.apply_rtlsdr_live_settings();
        self.apply_qmx_live_settings();
        self.apply_audio_device();
    }

    fn apply_kiwi_rf_attn_settings(&mut self) {
        if !self.kiwi_rf_live() {
            return;
        }
        if self.stats.kiwi_has_rf_attn && !self.last_kiwi_has_rf_attn {
            self.engine
                .send(EngineCommand::SetKiwiRfAttn(self.form_kiwi.rf_attn_db));
            self.last_kiwi_rf_attn_db = self.form_kiwi.rf_attn_db;
        }
        self.last_kiwi_has_rf_attn = self.stats.kiwi_has_rf_attn;
        if !self.stats.kiwi_has_rf_attn {
            return;
        }
        let db = self.form_kiwi.rf_attn_db;
        if (db - self.last_kiwi_rf_attn_db).abs() > 0.05 {
            self.engine.send(EngineCommand::SetKiwiRfAttn(db));
            self.last_kiwi_rf_attn_db = db;
            self.lock_display_levels_for_rf_tuning();
        }
    }

    fn apply_qmx_live_settings(&mut self) {
        #[cfg(not(feature = "qmx"))]
        {
            let _ = self;
            return;
        }
        #[cfg(feature = "qmx")]
        {
            if self.is_kiwi || !matches!(self.conn_state, ConnState::Streaming) {
                return;
            }
            if self.form_kind != SourceKind::Qmx {
                return;
            }
            if self.form_qmx.rf_gain_db != self.last_qmx_rf.rf_gain_db {
                self.engine
                    .send(EngineCommand::SetQmxRfGain(self.form_qmx.rf_gain_db));
                self.last_qmx_rf.rf_gain_db = self.form_qmx.rf_gain_db;
                self.lock_display_levels_for_rf_tuning();
            }
        }
    }

    fn apply_rtlsdr_live_settings(&mut self) {
        #[cfg(not(feature = "rtlsdr"))]
        {
            let _ = self;
            return;
        }
        #[cfg(feature = "rtlsdr")]
        {
            if self.is_kiwi || !matches!(self.conn_state, ConnState::Streaming) {
                return;
            }
            if self.form_kind != SourceKind::RtlSdr {
                return;
            }
            if self.form_rtlsdr.rtl_agc != self.last_rtlsdr_rf.rtl_agc {
                self.engine
                    .send(EngineCommand::SetRtlSdrRtlAgc(self.form_rtlsdr.rtl_agc));
                self.last_rtlsdr_rf.rtl_agc = self.form_rtlsdr.rtl_agc;
                self.lock_display_levels_for_rf_tuning();
            }
            if self.form_rtlsdr.manual_gain != self.last_rtlsdr_rf.manual_gain {
                self.engine
                    .send(EngineCommand::SetRtlSdrManualGain(self.form_rtlsdr.manual_gain));
                self.last_rtlsdr_rf.manual_gain = self.form_rtlsdr.manual_gain;
                self.lock_display_levels_for_rf_tuning();
            }
            if self.form_rtlsdr.manual_gain
                && self.form_rtlsdr.tuner_gain_db10 != self.last_rtlsdr_rf.tuner_gain_db10
            {
                self.engine.send(EngineCommand::SetRtlSdrTunerGain(
                    self.form_rtlsdr.tuner_gain_db10,
                ));
                self.last_rtlsdr_rf.tuner_gain_db10 = self.form_rtlsdr.tuner_gain_db10;
                self.lock_display_levels_for_rf_tuning();
            }
            if self.form_rtlsdr.bias_tee != self.last_rtlsdr_rf.bias_tee {
                self.engine
                    .send(EngineCommand::SetRtlSdrBiasTee(self.form_rtlsdr.bias_tee));
                self.last_rtlsdr_rf.bias_tee = self.form_rtlsdr.bias_tee;
            }
            if self.form_rtlsdr.ppm != self.last_rtlsdr_rf.ppm {
                self.engine
                    .send(EngineCommand::SetRtlSdrPpm(self.form_rtlsdr.ppm));
                self.last_rtlsdr_rf.ppm = self.form_rtlsdr.ppm;
            }
        }
    }

    fn apply_airspy_live_settings(&mut self) {
        #[cfg(not(feature = "airspy"))]
        {
            let _ = self;
            return;
        }
        #[cfg(feature = "airspy")]
        {
            if self.is_kiwi || !matches!(self.conn_state, ConnState::Streaming) {
                return;
            }
            if self.form_kind != SourceKind::Airspy {
                return;
            }
        if self.form_airspy.hf_agc != self.last_airspy_rf.hf_agc {
            self.engine
                .send(EngineCommand::SetRfAgc(self.form_airspy.hf_agc));
            self.last_airspy_rf.hf_agc = self.form_airspy.hf_agc;
            self.lock_display_levels_for_rf_tuning();
        }
        if self.form_airspy.hf_agc_threshold_high != self.last_airspy_rf.hf_agc_threshold_high {
            self.engine.send(EngineCommand::SetAirspyAgcThreshold(
                self.form_airspy.hf_agc_threshold_high,
            ));
            self.last_airspy_rf.hf_agc_threshold_high = self.form_airspy.hf_agc_threshold_high;
        }
        if self.form_airspy.hf_att != self.last_airspy_rf.hf_att {
            self.engine
                .send(EngineCommand::SetAirspyAtt(self.form_airspy.hf_att));
            self.last_airspy_rf.hf_att = self.form_airspy.hf_att;
            self.lock_display_levels_for_rf_tuning();
        }
        if self.form_airspy.hf_lna != self.last_airspy_rf.hf_lna {
            self.engine
                .send(EngineCommand::SetAirspyLna(self.form_airspy.hf_lna));
            self.last_airspy_rf.hf_lna = self.form_airspy.hf_lna;
            self.lock_display_levels_for_rf_tuning();
        }
        let frontend = self.form_airspy.frontend_flags();
        if frontend != self.last_airspy_rf.frontend_flags() {
            self.engine
                .send(EngineCommand::SetAirspyFrontendOptions(frontend));
            self.last_airspy_rf.frontend_optimize_band_iii =
                self.form_airspy.frontend_optimize_band_iii;
            self.last_airspy_rf.frontend_optimize_pll_boundary =
                self.form_airspy.frontend_optimize_pll_boundary;
        }
        if self.form_airspy.bias_tee != self.last_airspy_rf.bias_tee {
            self.engine
                .send(EngineCommand::SetAirspyBiasTee(self.form_airspy.bias_tee));
            self.last_airspy_rf.bias_tee = self.form_airspy.bias_tee;
        }
        }
    }

    fn apply_connect_form(&mut self, req: &ConnectRequest) {
        self.form_kind = req.kind;
        self.form_host = req.host.clone();
        self.form_port = req.port;
        self.form_kiwi = req.kiwi.clone();
        if req.kind == SourceKind::Kiwi {
            self.agc_rf_on = req.kiwi.rf_agc_on;
            self.last_agc_rf_on = req.kiwi.rf_agc_on;
        }
        if req.sample_rate != 0 {
            self.form_sample_rate = req.sample_rate;
        }
        self.form_airspy = req.airspy.clone();
        self.form_rtlsdr = req.rtlsdr.clone();
        self.form_qmx = req.qmx.clone();
    }

    fn can_connect_request(req: &ConnectRequest) -> bool {
        is_local_source(req.kind) || !req.host.trim().is_empty()
    }

    fn can_quick_connect(&self) -> bool {
        if let Some(req) = self.recent_hosts.first() {
            Self::can_connect_request(req)
        } else {
            is_local_source(self.form_kind) || !self.form_host.trim().is_empty()
        }
    }

    fn quick_connect_target_label(&self) -> String {
        self.recent_hosts
            .first()
            .map(|r| r.label())
            .unwrap_or_else(|| self.connection_alias())
    }

    fn quick_connect_last(&mut self) {
        if let Some(req) = self.recent_hosts.first().cloned() {
            self.apply_connect_form(&req);
        }
        self.connect_now();
    }

    fn connect_now(&mut self) {
        self.clamp_center_to_ham_bands();
        let sample_rate = match self.form_kind {
            #[cfg(feature = "airspy")]
            SourceKind::Airspy => self.form_sample_rate,
            #[cfg(feature = "rtlsdr")]
            SourceKind::RtlSdr => self.form_sample_rate,
            #[cfg(feature = "qmx")]
            SourceKind::Qmx => 0,
            _ => 0,
        };
        let mut kiwi = self.form_kiwi.clone();
        kiwi.rf_agc_on = self.agc_rf_on;
        let req = ConnectRequest {
            kind: self.form_kind,
            host: self.form_host.trim().to_string(),
            port: self.form_port,
            center_hz: self.center_khz * 1000.0,
            sample_rate,
            kiwi,
            airspy: self.form_airspy.clone(),
            rtlsdr: self.form_rtlsdr.clone(),
            qmx: self.form_qmx.clone(),
        };
        self.last_airspy_rf = self.form_airspy.clone();
        self.last_rtlsdr_rf = self.form_rtlsdr.clone();
        self.last_qmx_rf = self.form_qmx.clone();
        self.last_kiwi_man_gain = self.form_kiwi.man_gain;
        self.last_kiwi_rf_attn_db = self.form_kiwi.rf_attn_db;
        self.last_kiwi_has_rf_attn = false;
        self.last_agc_rf_on = !self.agc_rf_on;
        self.last_center_khz = self.center_khz;
        self.remember_host(&req);
        self.apply_default_view_zoom();
        log::info(format!("connecting to {}", req.label()));
        self.engine.send(EngineCommand::Connect(req));
    }

    fn cancel_connection(&mut self) {
        self.engine.abort_connect();
        self.engine.send(EngineCommand::Disconnect);
    }

    fn remember_host(&mut self, req: &ConnectRequest) {
        self.recent_hosts.retain(|r| r != req);
        self.recent_hosts.insert(0, req.clone());
        self.recent_hosts.truncate(8);
    }

    fn toggle_manual_notch(&mut self, slot: usize) {
        if slot >= MAX_NOTCHES {
            return;
        }
        if self.cw.notches[slot].enabled {
            self.cw.notches[slot].enabled = false;
        } else {
            self.arm_manual_notch(slot, None);
        }
    }

    /// ←/→ pan the spectrogram view when zoomed; otherwise nudge RX center.
    /// Tap = `pan_step_hz`, sustained hold accelerates (2× then fast), Shift = fine, Ctrl = fast.
    fn handle_arrow_pan(&mut self, ctx: &egui::Context) {
        use egui::Key;

        let (left_down, right_down, left_press, right_press, shift, ctrl) = ctx.input(|i| {
            (
                i.key_down(Key::ArrowLeft),
                i.key_down(Key::ArrowRight),
                i.key_pressed(Key::ArrowLeft),
                i.key_pressed(Key::ArrowRight),
                i.modifiers.shift,
                i.modifiers.ctrl || i.modifiers.command,
            )
        });

        if !left_down && !right_down {
            self.arrow_hold = None;
            return;
        }
        if !left_press && !right_press {
            return;
        }

        let direction = if left_press || (left_down && !right_down) {
            -1.0
        } else {
            1.0
        };

        let key = if direction < 0.0 {
            Key::ArrowLeft
        } else {
            Key::ArrowRight
        };
        let now = Instant::now();
        let hold = match self.arrow_hold {
            Some((held, started)) if held == key => now.saturating_duration_since(started),
            _ => {
                self.arrow_hold = Some((key, now));
                Duration::ZERO
            }
        };

        let base = self.pan_step_hz.max(10.0);
        let fast = self.pan_step_fast_hz.max(base);
        let step_hz = if ctrl {
            fast
        } else if shift {
            (base / 5.0).clamp(10.0, base)
        } else if hold >= Duration::from_millis(1200) {
            fast
        } else if hold >= Duration::from_millis(500) {
            (base * 2.0).clamp(base, fast)
        } else {
            base
        };

        let delta_hz = direction * step_hz as f64;
        let full_span = self.plot_full_span_hz();
        let max_zoom = self.plot_max_zoom_out();
        let can_pan = self.plot_view.can_pan(full_span, max_zoom);

        if can_pan || self.stats.iq_playback {
            self.plot_view.pan_offset_hz += delta_hz;
            self.plot_view.clamp_pan(full_span, max_zoom);
        } else {
            self.center_khz += delta_hz / 1000.0;
            self.clamp_center_to_ham_bands();
            self.apply_radio_settings();
        }
    }

    fn on_af_scope_panel_changed(&mut self) {
        if self.show_af_scope {
            self.show_right = true;
        }
    }

    fn toggle_af_scope(&mut self) {
        self.show_af_scope = !self.show_af_scope;
        self.on_af_scope_panel_changed();
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        self.handle_arrow_pan(ctx);
        let (
            zero,
            lock,
            notch,
            blank,
            nr,
            agc,
            apf,
            narrow,
            widen,
            rit_dn,
            rit_up,
            rit_clr,
            full,
            mute,
            vol_dn,
            vol_up,
            console,
            f11,
            overview,
            help,
            af_scope,
            notch1,
            notch2,
            notch3,
            notch4,
        ) = ctx.input(|i| {
            use egui::Key;
            (
                i.key_pressed(Key::Z),
                i.key_pressed(Key::L),
                i.key_pressed(Key::N),
                i.key_pressed(Key::B),
                i.key_pressed(Key::R),
                i.key_pressed(Key::A),
                i.key_pressed(Key::P),
                i.key_pressed(Key::OpenBracket),
                i.key_pressed(Key::CloseBracket),
                i.key_pressed(Key::Comma),
                i.key_pressed(Key::Period),
                i.key_pressed(Key::Backslash),
                i.key_pressed(Key::F),
                i.key_pressed(Key::Space),
                i.key_pressed(Key::Minus),
                i.key_pressed(Key::Equals),
                i.key_pressed(Key::Backtick),
                i.key_pressed(Key::F11),
                i.key_pressed(Key::M),
                i.key_pressed(Key::Questionmark),
                i.key_pressed(Key::G),
                i.key_pressed(Key::Num1),
                i.key_pressed(Key::Num2),
                i.key_pressed(Key::Num3),
                i.key_pressed(Key::Num4),
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
        if apf {
            self.cw.apf.enabled = !self.cw.apf.enabled;
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
        if rit_clr {
            self.clear_rit();
        }
        if full {
            self.plot_view.zoom_to_full_span();
        }
        if mute {
            self.audio_enabled = !self.audio_enabled;
        }
        if vol_dn {
            self.volume = (self.volume - 0.1).max(0.0);
        }
        if vol_up {
            self.volume = (self.volume + 0.1).min(4.0);
        }
        if console {
            self.show_console = !self.show_console;
        }
        if f11 {
            let on = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!on));
        }
        if overview {
            self.show_band_overview = !self.show_band_overview;
        }
        if help {
            self.show_shortcuts = !self.show_shortcuts;
        }
        if af_scope {
            self.toggle_af_scope();
        }
        if notch1 {
            self.toggle_manual_notch(0);
        }
        if notch2 {
            self.toggle_manual_notch(1);
        }
        if notch3 {
            self.toggle_manual_notch(2);
        }
        if notch4 {
            self.toggle_manual_notch(3);
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

    fn waterfall_source_row(&self, row_index: usize) -> Option<&[f32]> {
        if let Some(row) = self.rows.get(row_index) {
            return Some(row.as_slice());
        }
        // Only the newest row may fall back to the live FFT; older slots stay empty until
        // history refills so a tune reset cannot paint the whole waterfall as one column.
        (row_index == 0 && !self.latest.is_empty()).then(|| self.latest.as_slice())
    }

    fn write_row_pixels(
        pixels: &mut [Color32],
        y: usize,
        width: usize,
        db_row: &[f32],
        ref_db: f32,
        range_db: f32,
    ) {
        let base = y * width;
        for (x, &db) in db_row.iter().enumerate().take(width) {
            pixels[base + x] = db_to_colour(db, ref_db, range_db);
        }
    }

    fn waterfall_row_db_for_storage(
        &self,
        row_index: usize,
        storage: &SpectrumViewMapping,
        width: usize,
        avg: usize,
    ) -> Vec<f32> {
        let mut acc = vec![0.0f32; width];
        let mut count = 0usize;
        for k in 0..avg {
            let Some(row_data) = self.waterfall_source_row(row_index.saturating_add(k)) else {
                break;
            };
            let row = compose_panadapter_row(
                row_data,
                storage.row_rate_hz,
                storage.view_span_hz,
                storage.data_span_hz,
                storage.compose_pan_offset_hz,
                storage.allow_band_padding,
            );
            let n = row.len().min(width);
            for (i, &v) in row.iter().take(n).enumerate() {
                acc[i] += v;
            }
            count += 1;
        }
        if count == 0 {
            return vec![-120.0; width];
        }
        let inv = 1.0 / count as f32;
        for v in &mut acc {
            *v *= inv;
        }
        acc
    }

    fn waterfall_row_db_for_viewport(
        &self,
        row_index: usize,
        view: &SpectrumViewMapping,
        width: usize,
        avg: usize,
    ) -> Vec<f32> {
        let mut acc = vec![0.0f32; width.max(1)];
        let mut count = 0usize;
        for k in 0..avg {
            let Some(row_data) = self.waterfall_source_row(row_index.saturating_add(k)) else {
                break;
            };
            let row = compose_panadapter_row(
                row_data,
                view.row_rate_hz,
                view.view_span_hz,
                view.data_span_hz,
                view.compose_pan_offset_hz,
                view.allow_band_padding,
            );
            let stretched = stretch_row_to_width(&row, width);
            let n = stretched.len().min(width);
            for (i, &v) in stretched.iter().take(n).enumerate() {
                acc[i] += v;
            }
            count += 1;
        }
        if count == 0 {
            return vec![-120.0; width.max(1)];
        }
        let inv = 1.0 / count as f32;
        for v in &mut acc {
            *v *= inv;
        }
        acc
    }

    fn upload_waterfall_viewport(&mut self, ctx: &egui::Context, width: usize, height: usize) {
        let image = egui::ColorImage::new([width, height], self.waterfall_viewport_pixels.clone());
        match &mut self.waterfall_viewport_texture {
            Some(tex) => tex.set(image, egui::TextureOptions::NEAREST),
            none => {
                *none = Some(ctx.load_texture(
                    "waterfall_viewport",
                    image,
                    egui::TextureOptions::NEAREST,
                ));
            }
        }
    }

    fn sync_waterfall_storage(&mut self, ctx: &egui::Context) {
        if self.rows.is_empty() {
            return;
        }
        let storage = self.waterfall_storage_view();
        let row_len = self
            .rows
            .front()
            .map(|r| r.len())
            .unwrap_or_else(|| self.latest.len());
        if row_len == 0 {
            return;
        }
        let w = self.storage_row_width(&storage, row_len);
        let h = WATERFALL_ROWS;
        let key = StorageKey::from(&storage, w);
        let avg = self.waterfall_avg.max(1) as usize;
        let ref_db = self.ref_db;
        let range_db = self.range_db;
        let n_new = self.pending_row_appends.min(h);
        let can_append = n_new > 0
            && n_new < h
            && !self.force_texture_full
            && self.last_storage_key == Some(key)
            && self.storage_tex_width == w
            && self.waterfall_storage_pixels.len() == w * h;

        if can_append {
            let stride = w;
            for y in (0..h - n_new).rev() {
                let src = y * stride;
                self.waterfall_storage_pixels
                    .copy_within(src..src + stride, (y + n_new) * stride);
            }
            for y in 0..n_new {
                self.waterfall_row_scratch =
                    self.waterfall_row_db_for_storage(y, &storage, w, avg);
                Self::write_row_pixels(
                    &mut self.waterfall_storage_pixels,
                    y,
                    w,
                    &self.waterfall_row_scratch,
                    ref_db,
                    range_db,
                );
            }
        } else if self.textures_dirty
            || self.force_texture_full
            || self.last_storage_key != Some(key)
            || self.storage_tex_width != w
            || self.waterfall_storage_pixels.len() != w * h
        {
            self.storage_tex_width = w;
            self.waterfall_storage_pixels.resize(w * h, Color32::BLACK);
            for y in 0..h {
                let row_db = self.waterfall_row_db_for_storage(y, &storage, w, avg);
                Self::write_row_pixels(
                    &mut self.waterfall_storage_pixels,
                    y,
                    w,
                    &row_db,
                    ref_db,
                    range_db,
                );
            }
            self.last_storage_key = Some(key);
            self.last_viewport_key = None;
        } else {
            return;
        }

        self.textures_dirty = false;
        self.pending_row_appends = 0;
        let _ = ctx; // storage is CPU-side; viewport upload happens in sync_waterfall_viewport
    }

    fn sync_waterfall_viewport(&mut self, ctx: &egui::Context, plot_width: usize) {
        if self.rows.is_empty() {
            return;
        }
        let view = self.spectrum_view();
        let dst_w = plot_width.max(1);
        let h = WATERFALL_ROWS;
        let key = ViewportKey::from_view(view.view_span_hz, view.pan_offset_hz, dst_w);
        let avg = self.waterfall_avg.max(1) as usize;
        let ref_db = self.ref_db;
        let range_db = self.range_db;

        let n_new = self.pending_viewport_row_appends.min(h);
        let can_append = n_new > 0
            && n_new < h
            && !self.force_texture_full
            && !self.textures_dirty
            && self.last_viewport_key == Some(key)
            && self.waterfall_viewport_texture.is_some()
            && self.viewport_tex_width == dst_w
            && self.waterfall_viewport_pixels.len() == dst_w * h;

        if can_append {
            let stride = dst_w;
            for y in (0..h - n_new).rev() {
                let src = y * stride;
                self.waterfall_viewport_pixels
                    .copy_within(src..src + stride, (y + n_new) * stride);
            }
            for y in 0..n_new {
                self.waterfall_row_scratch =
                    self.waterfall_row_db_for_viewport(y, &view, dst_w, avg);
                Self::write_row_pixels(
                    &mut self.waterfall_viewport_pixels,
                    y,
                    dst_w,
                    &self.waterfall_row_scratch,
                    ref_db,
                    range_db,
                );
            }
            self.upload_waterfall_viewport(ctx, dst_w, h);
            self.pending_viewport_row_appends = 0;
            return;
        }

        if self.last_viewport_key == Some(key)
            && self.waterfall_viewport_texture.is_some()
            && self.viewport_tex_width == dst_w
            && self.waterfall_viewport_pixels.len() == dst_w * h
            && !self.textures_dirty
            && !self.force_texture_full
        {
            self.pending_viewport_row_appends = 0;
            return;
        }

        self.waterfall_viewport_pixels.resize(dst_w * h, Color32::BLACK);
        for y in 0..h {
            let row_db = self.waterfall_row_db_for_viewport(y, &view, dst_w, avg);
            Self::write_row_pixels(
                &mut self.waterfall_viewport_pixels,
                y,
                dst_w,
                &row_db,
                ref_db,
                range_db,
            );
        }
        self.viewport_tex_width = dst_w;
        self.upload_waterfall_viewport(ctx, dst_w, h);
        self.last_viewport_key = Some(key);
        self.pending_viewport_row_appends = 0;
        self.force_texture_full = false;
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

    fn connection_status_pill(&self) -> (String, Color32) {
        match &self.conn_state {
            ConnState::Streaming if self.connection_unstable() => ("UNSTABLE".to_string(), WARN),
            ConnState::Streaming => ("STREAMING".to_string(), OK),
            ConnState::Reconnecting { attempt, retry_in_s } => {
                (format!("RECONNECT #{attempt} ({retry_in_s:.0}s)"), WARN)
            }
            ConnState::Connecting { .. } => ("CONNECTING".to_string(), WARN),
            ConnState::Disconnected => ("OFFLINE".to_string(), MUTED),
        }
    }

    fn connection_session_live(&self) -> bool {
        matches!(
            self.conn_state,
            ConnState::Streaming | ConnState::Connecting { .. } | ConnState::Reconnecting { .. }
        )
    }

    fn connection_alias(&self) -> String {
        match self.form_kind {
            #[cfg(feature = "airspy")]
            SourceKind::Airspy => "Airspy HF+".to_string(),
            #[cfg(feature = "rtlsdr")]
            SourceKind::RtlSdr => format!("RTL-SDR #{}", self.form_rtlsdr.device_index),
            #[cfg(feature = "qmx")]
            SourceKind::Qmx => {
                if self.form_qmx.serial_port.is_empty() {
                    "QMX".to_string()
                } else {
                    format!("QMX ({})", self.form_qmx.serial_port)
                }
            }
            SourceKind::Kiwi => {
                let host = self.form_host.trim();
                if host.is_empty() {
                    "KiwiSDR".to_string()
                } else {
                    format!("{host}:{}", self.form_port)
                }
            }
        }
    }

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

    fn pipeline_ingress_decim(&self) -> usize {
        let device_rate = if self.stats.sample_rate > 0.0 {
            self.stats.sample_rate.round() as u32
        } else {
            self.form_sample_rate
        };
        match self.form_kind {
            #[cfg(feature = "airspy")]
            SourceKind::Airspy => self.form_airspy.ingress_decimation(device_rate).0,
            SourceKind::Kiwi => self.form_kiwi.ingress_decimation(device_rate).0,
            #[cfg(feature = "rtlsdr")]
            SourceKind::RtlSdr => self.form_rtlsdr.ingress_decimation(device_rate).0,
            #[cfg(feature = "qmx")]
            SourceKind::Qmx => self.form_qmx.ingress_decimation(device_rate).0,
        }
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

    fn process_iq_cmds(&mut self, cmds: Vec<IqPanelCmd>) {
        for cmd in cmds {
            match cmd {
                IqPanelCmd::StartRecord(path) => {
                    self.engine.send(EngineCommand::StartIqRecord(path));
                }
                IqPanelCmd::StopRecord => {
                    self.engine.send(EngineCommand::StopIqRecord);
                }
                IqPanelCmd::Play(path) => {
                    if let Ok(meta) = hfsdr::read_meta(&path) {
                        self.center_khz = meta.center_hz / 1000.0;
                        self.plot_view.pan_offset_hz = 0.0;
                        self.clear_rit();
                    }
                    self.engine.send(EngineCommand::PlayIqFile(path));
                }
                IqPanelCmd::StopPlayback => {
                    self.engine.send(EngineCommand::StopIqPlayback);
                }
            }
        }
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
        let streaming = matches!(self.conn_state, ConnState::Streaming);
        let rx_color = if streaming { ACCENT } else { MUTED };

        ui.horizontal(|ui| {
            let badge_resp = clickable_badge(ui, &conn_label, conn_color)
                .on_hover_text("Click to open connection settings");
            if badge_resp.clicked() {
                self.show_connection_drawer = !self.show_connection_drawer;
            }
            if self.connection_session_live() {
                let alias_resp =
                    crate::status_widgets::connection_alias_chip(ui, &self.connection_alias());
                if alias_resp.clicked() {
                    self.show_connection_drawer = !self.show_connection_drawer;
                }
                if crate::status_widgets::disconnect_chip(ui).clicked() {
                    self.cancel_connection();
                }
            } else if matches!(self.conn_state, ConnState::Disconnected) {
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
                egui::RichText::new(format!("RX {:.6} MHz", self.center_khz / 1000.0))
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
                egui::RichText::new(format!("SNR {:.0} dB", self.last_snr_db))
                    .small()
                    .color(MUTED),
            );
            let (cursor_label, cursor_active) = if let Some(offset) = self.hover_offset_hz {
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
                self.show_pipeline_drawer,
                streaming,
            );
            if engine_resp.clicked() {
                self.show_pipeline_drawer = !self.show_pipeline_drawer;
            }
            let gauge_resp = crate::status_widgets::iq_buffer_control(
                ui,
                self.stats.iq_buffer_fill,
                self.stats.iq_buffer_secs,
                self.show_iq_drawer,
            );
            if gauge_resp.clicked() {
                self.show_iq_drawer = !self.show_iq_drawer;
            }
            let rec_secs = self.stats.iq_capture_samples as f32 / self.stats.sample_rate.max(1.0);
            let rec_resp = crate::status_widgets::iq_record_toggle(
                ui,
                self.stats.iq_recording,
                streaming,
                rec_secs,
            );
            if rec_resp.clicked() {
                if let Some(cmd) = self.iq.toggle_recording(self.stats.iq_recording, streaming) {
                    self.settings_dirty_at = Some(Instant::now());
                    self.process_iq_cmds(vec![cmd]);
                }
            }
            let has_iq_file = !self.iq.playback_path.trim().is_empty();
            let play_resp = crate::status_widgets::iq_playback_chip(
                ui,
                self.stats.iq_playback,
                has_iq_file,
            );
            if play_resp.clicked() {
                if let Some(cmd) = self.iq.replay_playback() {
                    self.process_iq_cmds(vec![cmd]);
                }
            }
            ui.label(
                egui::RichText::new(format!("{:.0} kS/s", self.stats.effective_sps / 1000.0))
                    .small()
                    .color(MUTED),
            );
            if !self.is_kiwi
                && self.stats.sample_rate > 0.0
                && (self.stats.effective_sps / self.stats.sample_rate) < 0.85
            {
                ui.label(
                    egui::RichText::new(format!(
                        "({:.0} kS/s device)",
                        self.stats.sample_rate / 1000.0
                    ))
                    .small()
                    .color(MUTED),
                );
            }
            if self.stats.iq_playback {
                ui.colored_label(OK, "PLAYBACK");
            }
            if self.stats.dropped > 0 {
                ui.colored_label(WARN, format!("drops {}", self.stats.dropped));
            }
            if streaming && !(self.show_left && self.show_smeter) {
                show_status_rf_meter(
                    ui,
                    self.rf_meter_dbm(),
                    self.stats.rssi_dbm,
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
                    self.show_shortcuts = !self.show_shortcuts;
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
                panel_toggle(ui, &mut self.show_console, "Log", "Application log (`)");
                panel_toggle(ui, &mut self.show_history, "Spots", "Decoded callsign history");
                if panel_toggle(
                    ui,
                    &mut self.show_af_scope,
                    "Scope",
                    "AF scope for RF gain tuning (G)",
                ) {
                    self.on_af_scope_panel_changed();
                }
                panel_toggle(
                    ui,
                    &mut self.show_smeter,
                    "Meter",
                    "S-meter and IF/AF AGC levels",
                );
                panel_toggle(ui, &mut self.show_right, "DSP", "CW demod, skimmer, audio, display");
                panel_toggle(ui, &mut self.show_left, "RX", "VFO, RF gains, IQ chain");
            });
        });

        if let Some(err) = &self.last_error {
            if matches!(self.conn_state, ConnState::Reconnecting { .. }) {
                ui.colored_label(WARN, err);
            }
        }
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

    fn side_panel_scroll(&mut self, ui: &mut egui::Ui, mut body: impl FnMut(&mut Self, &mut egui::Ui)) {
        let panel_w = ui.max_rect().width();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_min_width(panel_w);
                body(self, ui);
            });
    }

    fn left_panel(&mut self, ui: &mut egui::Ui) {
        self.side_panel_scroll(ui, |this, ui| {
            if this.show_smeter {
                this.smeter_card(ui);
            }
            if !this.show_left {
                return;
            }
            this.frequency_card(ui);
            this.rf_front_end_card(ui);
            this.receive_chain_card(ui);
        });
    }

    fn right_panel(&mut self, ui: &mut egui::Ui) {
        self.side_panel_scroll(ui, |this, ui| {
            this.af_tuning_card(ui);
            this.cw_demod_card(ui);
            this.display_section(ui);
            this.spot_display_section(ui);
            collapsible_section(ui, "skimmer-settings", "Skimmer settings", None, false, |ui| {
                this.skimmer_settings_body(ui);
            });
            collapsible_section(ui, "audio", "Audio", None, false, |ui| {
                this.audio_card_body(ui);
            });
            this.performance_section(ui);
        });
    }

    fn spot_display_section(&mut self, ui: &mut egui::Ui) {
        collapsible_section(ui, "spots", "Spots", None, false, |ui| {
            self.spot_display_body(ui);
        });
    }

    fn spot_display_body(&mut self, ui: &mut egui::Ui) {
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
            } else if !self.skimmer_spectrum_ok() {
                ui.colored_label(
                    WARN,
                    "Skimmer needs Process IQ ≤96 kHz on Airspy (Connection → Process IQ), then reconnect.",
                );
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
    }

    fn connection_card(&mut self, ui: &mut egui::Ui) {
        let connecting = matches!(
            self.conn_state,
            ConnState::Connecting { .. } | ConnState::Reconnecting { .. }
        );
        if self.connection_unstable() {
            alert_banner(ui, "Link unstable — tuning kept", self.last_error.as_deref());
            if connecting {
                section_hint(ui, "Click Cancel to stop the current attempt and disable auto-reconnect.");
            }
        }

        popup_section(ui, "Connect", None, |ui| {
            let labels = source_kind_labels();
            let selected = source_kind_index(self.form_kind);
            if let Some(i) = segment_choice(ui, "source_kind", selected, &labels) {
                self.form_kind = source_kind_from_index(i);
            }

            if self.form_kind == SourceKind::Kiwi {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Host").small().color(MUTED));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.form_host)
                            .hint_text("kiwi.example.com")
                            .desired_width(ui.available_width() - 72.0),
                    );
                    ui.label(egui::RichText::new("Port").small().color(MUTED));
                    ui.add(egui::DragValue::new(&mut self.form_port).range(1..=65535));
                });
            }

            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("RX {:.6} MHz", self.center_khz / 1000.0))
                        .small()
                        .monospace()
                        .color(MUTED),
                );
            });

            let session_active = self.connection_session_live();
            let can_connect = is_local_source(self.form_kind) || !self.form_host.trim().is_empty();
            ui.horizontal(|ui| {
                if primary_button(ui, "Connect", can_connect && !session_active).clicked() {
                    self.connect_now();
                }
                if session_active {
                    let label = if connecting { "Cancel" } else { "Disconnect" };
                    if secondary_button(ui, label)
                        .on_hover_text(if connecting {
                            "Stop connecting and cancel auto-retry"
                        } else {
                            "Disconnect from the receiver"
                        })
                        .clicked()
                    {
                        self.cancel_connection();
                    }
                }
            });
        });

        #[cfg(feature = "airspy")]
        if self.form_kind == SourceKind::Airspy {
            popup_section(ui, "Airspy HF+", None, |ui| {
                egui::Grid::new("connect_airspy_grid")
                    .num_columns(2)
                    .spacing([8.0, 4.0])
                    .min_col_width(100.0)
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new("Sample rate").small().color(MUTED));
                        preset_combo_u32(
                            ui,
                            "airspy_sr",
                            &mut self.form_sample_rate,
                            AIRSPY_SAMPLE_RATE_PRESETS,
                            "Hz ",
                            3_000..=768_000,
                        );
                        ui.end_row();

                        ui.label(egui::RichText::new("Process IQ").small().color(MUTED));
                        preset_combo_u32(
                            ui,
                            "airspy_proc",
                            &mut self.form_airspy.iq_process_hz,
                            AIRSPY_PROCESS_RATE_PRESETS,
                            "Hz ",
                            0..=768_000,
                        );
                        ui.end_row();

                        ui.label(egui::RichText::new("HF AGC").small().color(MUTED));
                        ui.toggle_value(&mut self.form_airspy.hf_agc, "On");
                        ui.end_row();

                        ui.label(egui::RichText::new("AGC threshold").small().color(MUTED));
                        ui.horizontal(|ui| {
                            ui.selectable_value(
                                &mut self.form_airspy.hf_agc_threshold_high,
                                false,
                                "Low",
                            );
                            ui.selectable_value(
                                &mut self.form_airspy.hf_agc_threshold_high,
                                true,
                                "High",
                            );
                        });
                        ui.end_row();

                        ui.label(egui::RichText::new("Attenuator").small().color(MUTED));
                        ui.add(
                            egui::DragValue::new(&mut self.form_airspy.hf_att)
                                .range(0..=8)
                                .suffix(" ×6 dB"),
                        );
                        ui.end_row();

                        ui.label(egui::RichText::new("Preamp").small().color(MUTED));
                        ui.toggle_value(&mut self.form_airspy.hf_lna, "+6 dB LNA (passive ant.)");
                        ui.end_row();

                        ui.label(egui::RichText::new("Bias tee").small().color(MUTED));
                        ui.toggle_value(
                            &mut self.form_airspy.bias_tee,
                            "Antenna DC (active preamp)",
                        );
                        ui.end_row();

                        ui.label(egui::RichText::new("Frontend").small().color(MUTED));
                        ui.vertical(|ui| {
                            ui.toggle_value(
                                &mut self.form_airspy.frontend_optimize_band_iii,
                                "Optimize VHF Band III",
                            );
                            ui.toggle_value(
                                &mut self.form_airspy.frontend_optimize_pll_boundary,
                                "Optimize PLL int. boundary",
                            );
                        });
                        ui.end_row();

                        ui.label(egui::RichText::new("Lib DSP").small().color(MUTED));
                        ui.toggle_value(&mut self.form_airspy.lib_dsp, "IQ correction");
                        ui.end_row();
                    });
                section_hint(
                    ui,
                    "384 kHz is a good CW default. Lower “Process IQ” cuts CPU load (reconnect). \
                     Preamp/Att/AGC apply live when connected. Discovery HF+ band-tracking \
                     preselectors are automatic — no manual filter-bank setting in libairspyhf.",
                );
            });
        }

        #[cfg(feature = "rtlsdr")]
        if self.form_kind == SourceKind::RtlSdr {
            popup_section(ui, "RTL-SDR", None, |ui| {
                egui::Grid::new("connect_rtlsdr_grid")
                    .num_columns(2)
                    .spacing([8.0, 4.0])
                    .min_col_width(100.0)
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new("Device").small().color(MUTED));
                        ui.add(
                            egui::DragValue::new(&mut self.form_rtlsdr.device_index)
                                .range(0..=15)
                                .speed(0.1),
                        );
                        ui.end_row();

                        ui.label(egui::RichText::new("Sample rate").small().color(MUTED));
                        preset_combo_u32(
                            ui,
                            "rtlsdr_sr",
                            &mut self.form_sample_rate,
                            RTLSDR_SAMPLE_RATE_PRESETS,
                            "Hz ",
                            250_000..=3_200_000,
                        );
                        ui.end_row();

                        ui.label(egui::RichText::new("Process IQ").small().color(MUTED));
                        preset_combo_u32(
                            ui,
                            "rtlsdr_proc",
                            &mut self.form_rtlsdr.iq_process_hz,
                            RTLSDR_PROCESS_RATE_PRESETS,
                            "Hz ",
                            0..=3_200_000,
                        );
                        ui.end_row();

                        ui.label(egui::RichText::new("PPM").small().color(MUTED));
                        ui.add(
                            egui::DragValue::new(&mut self.form_rtlsdr.ppm)
                                .range(-200..=200)
                                .speed(0.1),
                        );
                        ui.end_row();

                        ui.label(egui::RichText::new("RTL AGC").small().color(MUTED));
                        ui.toggle_value(&mut self.form_rtlsdr.rtl_agc, "On");
                        ui.end_row();

                        ui.label(egui::RichText::new("Manual gain").small().color(MUTED));
                        ui.toggle_value(&mut self.form_rtlsdr.manual_gain, "On");
                        ui.end_row();

                        if self.form_rtlsdr.manual_gain {
                            ui.label(egui::RichText::new("Tuner gain").small().color(MUTED));
                            ui.add(
                                egui::DragValue::new(&mut self.form_rtlsdr.tuner_gain_db10)
                                    .range(0..=500)
                                    .speed(0.5)
                                    .suffix(" ×0.1 dB"),
                            );
                            ui.end_row();
                        }

                        ui.label(egui::RichText::new("Direct sampling").small().color(MUTED));
                        ui.horizontal(|ui| {
                            ui.selectable_value(&mut self.form_rtlsdr.direct_sampling, 0, "Off");
                            ui.selectable_value(&mut self.form_rtlsdr.direct_sampling, 1, "I");
                            ui.selectable_value(&mut self.form_rtlsdr.direct_sampling, 2, "Q");
                        });
                        ui.end_row();

                        ui.label(egui::RichText::new("Offset tune").small().color(MUTED));
                        ui.toggle_value(&mut self.form_rtlsdr.offset_tuning, "On");
                        ui.end_row();

                        ui.label(egui::RichText::new("Bias tee").small().color(MUTED));
                        ui.toggle_value(&mut self.form_rtlsdr.bias_tee, "GPIO DC");
                        ui.end_row();
                    });
                section_hint(
                    ui,
                    "2.048 MHz suits HF with an upconverter. Use direct sampling for 0–28.8 MHz IF \
                     (Q branch often quieter). Lower “Process IQ” to ≤96 kHz for skimmer (reconnect). \
                     Gain / bias / PPM apply live when connected.",
                );
            });
        }

        #[cfg(feature = "qmx")]
        if self.form_kind == SourceKind::Qmx {
            let serial_ports = hfsdr::qmx::list_serial_ports();
            let audio_inputs = hfsdr::qmx::list_input_devices();
            popup_section(ui, "QMX / QMX+", None, |ui| {
                egui::Grid::new("connect_qmx_grid")
                    .num_columns(2)
                    .spacing([8.0, 4.0])
                    .min_col_width(100.0)
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new("CAT port").small().color(MUTED));
                        egui::ComboBox::from_id_salt("qmx_serial")
                            .selected_text(if self.form_qmx.serial_port.is_empty() {
                                "(first available)".to_string()
                            } else {
                                self.form_qmx.serial_port.clone()
                            })
                            .width(ui.available_width())
                            .show_ui(ui, |ui| {
                                if ui
                                    .selectable_label(self.form_qmx.serial_port.is_empty(), "(first available)")
                                    .clicked()
                                {
                                    self.form_qmx.serial_port.clear();
                                }
                                for port in &serial_ports {
                                    if ui
                                        .selectable_label(
                                            self.form_qmx.serial_port == *port,
                                            port,
                                        )
                                        .clicked()
                                    {
                                        self.form_qmx.serial_port = port.clone();
                                    }
                                }
                            });
                        ui.end_row();

                        ui.label(egui::RichText::new("IQ audio in").small().color(MUTED));
                        egui::ComboBox::from_id_salt("qmx_audio")
                            .selected_text(if self.form_qmx.audio_device.is_empty() {
                                "(auto-detect QMX)".to_string()
                            } else {
                                self.form_qmx.audio_device.clone()
                            })
                            .width(ui.available_width())
                            .show_ui(ui, |ui| {
                                if ui
                                    .selectable_label(self.form_qmx.audio_device.is_empty(), "(auto-detect QMX)")
                                    .clicked()
                                {
                                    self.form_qmx.audio_device.clear();
                                }
                                for dev in &audio_inputs {
                                    if ui
                                        .selectable_label(
                                            self.form_qmx.audio_device == *dev,
                                            dev,
                                        )
                                        .clicked()
                                    {
                                        self.form_qmx.audio_device = dev.clone();
                                    }
                                }
                            });
                        ui.end_row();

                        ui.label(egui::RichText::new("Process IQ").small().color(MUTED));
                        preset_combo_u32(
                            ui,
                            "qmx_proc",
                            &mut self.form_qmx.iq_process_hz,
                            QMX_PROCESS_RATE_PRESETS,
                            "Hz ",
                            0..=48_000,
                        );
                        ui.end_row();

                        ui.label(egui::RichText::new("IF offset").small().color(MUTED));
                        ui.add(
                            egui::DragValue::new(&mut self.form_qmx.if_offset_hz)
                                .range(0..=50_000)
                                .suffix(" Hz"),
                        );
                        ui.end_row();

                        ui.label(egui::RichText::new("RF gain").small().color(MUTED));
                        ui.add(
                            egui::DragValue::new(&mut self.form_qmx.rf_gain_db)
                                .range(0..=99)
                                .suffix(" dB"),
                        );
                        ui.end_row();

                        ui.label(egui::RichText::new("CAT timeout").small().color(MUTED));
                        ui.toggle_value(
                            &mut self.form_qmx.disable_cat_timeout,
                            "Disable (stay in RX)",
                        );
                        ui.end_row();

                        ui.label(egui::RichText::new("CW mode").small().color(MUTED));
                        ui.toggle_value(&mut self.form_qmx.force_cw_mode, "Set at connect");
                        ui.end_row();
                    });
                section_hint(
                    ui,
                    "IQ is 48 kHz stereo USB audio (I=left, Q=right). CAT enables IQ mode (Q9) \
                     and tunes VFO A (FA). The 12 kHz IF offset is applied automatically. \
                     RF gain applies live when connected; port/audio choices need reconnect.",
                );
            });
        }

        if self.form_kind == SourceKind::Kiwi {
            popup_section(ui, "Kiwi IQ", None, |ui| {
                egui::Grid::new("connect_kiwi_grid")
                    .num_columns(2)
                    .spacing([8.0, 4.0])
                    .min_col_width(100.0)
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new("IQ rate").small().color(MUTED));
                        preset_combo_u32(
                            ui,
                            "kiwi_iq_rate",
                            &mut self.form_kiwi.iq_rate_hz,
                            KIWI_IQ_RATE_PRESETS,
                            "Hz ",
                            4_000..=30_000,
                        );
                        ui.end_row();

                        ui.label(egui::RichText::new("Bandwidth").small().color(MUTED));
                        preset_combo_u32(
                            ui,
                            "kiwi_bw",
                            &mut self.form_kiwi.iq_half_bw_hz,
                            KIWI_BW_PRESETS,
                            "±Hz ",
                            0..=30_000,
                        );
                        ui.end_row();

                        ui.label(egui::RichText::new("Resample").small().color(MUTED));
                        preset_combo_u32(
                            ui,
                            "kiwi_resample",
                            &mut self.form_kiwi.iq_resample_hz,
                            KIWI_RESAMPLE_PRESETS,
                            "Hz ",
                            0..=48_000,
                        );
                        ui.end_row();

                        ui.label(egui::RichText::new("LNB LO").small().color(MUTED));
                        preset_combo_f64(
                            ui,
                            "kiwi_lo",
                            &mut self.form_kiwi.freq_offset_khz,
                            KIWI_LO_PRESETS,
                            "kHz ",
                            0.0..=1_000_000.0,
                        );
                        ui.end_row();

                        ui.label(egui::RichText::new("AR out").small().color(MUTED));
                        preset_combo_u32(
                            ui,
                            "kiwi_ar",
                            &mut self.form_kiwi.ar_out_hz,
                            KIWI_AR_OUT_PRESETS,
                            "Hz ",
                            8_000..=192_000,
                        );
                        ui.end_row();

                        ui.label(egui::RichText::new("RF attn").small().color(MUTED));
                        ui.add(
                            egui::DragValue::new(&mut self.form_kiwi.rf_attn_db)
                                .range(0.0..=31.5)
                                .speed(0.1)
                                .suffix(" dB"),
                        );
                        ui.end_row();

                        ui.label(egui::RichText::new("Gen attn").small().color(MUTED));
                        ui.add(
                            egui::DragValue::new(&mut self.form_kiwi.gen_attn)
                                .range(0..=255)
                                .suffix(" (handshake)"),
                        );
                        ui.end_row();
                    });
            });

            popup_section(ui, "Public KiwiSDRs", None, |ui| {
                if self.kiwi_directory_rx.is_some() {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(egui::RichText::new("Loading…").small().color(MUTED));
                    });
                } else if !self.kiwi_nearby.is_empty() {
                    let mut nearby = self.kiwi_nearby.clone();
                    nearby.sort_by(|a, b| {
                        let af = a.users >= a.users_max;
                        let bf = b.users >= b.users_max;
                        af.cmp(&bf).then_with(|| {
                            a.distance_km
                                .partial_cmp(&b.distance_km)
                                .unwrap_or(std::cmp::Ordering::Equal)
                        })
                    });
                    egui::ScrollArea::vertical()
                        .max_height(130.0)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            for rx in nearby {
                                let full = rx.users >= rx.users_max;
                                let dist = if rx.distance_km > 0.0 {
                                    format!("{:.0}km ", rx.distance_km)
                                } else {
                                    String::new()
                                };
                                let users = if full {
                                    format!("FULL {}/{}", rx.users, rx.users_max)
                                } else {
                                    format!("{}/{}", rx.users, rx.users_max)
                                };
                                let line = format!(
                                    "{}:{} · {}{} · {}",
                                    rx.host, rx.port, dist, users, rx.location
                                );
                                let resp = list_row(ui, &line, !full);
                                if resp.clicked() {
                                    self.form_host = rx.host;
                                    self.form_port = rx.port;
                                    self.connect_now();
                                }
                            }
                        });
                    if ghost_button(ui, "Refresh").clicked() {
                        self.start_kiwi_directory_fetch(true);
                    }
                } else if let Some(err) = &self.kiwi_directory_error {
                    alert_banner(ui, err, None);
                    if ghost_button(ui, "Retry").clicked() {
                        self.kiwi_directory_error = None;
                        self.start_kiwi_directory_fetch(true);
                    }
                } else if ghost_button(ui, "Refresh").clicked() {
                    self.start_kiwi_directory_fetch(true);
                }
            });
        }

        if !self.recent_hosts.is_empty() {
            popup_section(ui, "Recent", None, |ui| {
                let labels: Vec<String> = self.recent_hosts.iter().map(|r| r.label()).collect();
                let recents = self.recent_hosts.clone();
                if let Some(i) = chip_row(ui, &labels) {
                    let req = &recents[i];
                    self.apply_connect_form(req);
                    self.connect_now();
                }
            });
        }

        if let Some(err) = &self.last_error {
            if matches!(
                self.conn_state,
                ConnState::Reconnecting { .. } | ConnState::Connecting { .. }
            ) {
                alert_banner(ui, err, None);
            }
        }

        let mut stats = vec![
            (
                "rate",
                format!("{:.1} kS/s", self.stats.effective_sps / 1000.0),
            ),
            ("drops", self.stats.dropped.to_string()),
        ];
        if let Some(rssi) = self.stats.rssi_dbm {
            stats.push(("S", format!("{rssi:.0} dBm")));
        }
        inline_stats(ui, &stats);
    }

    fn display_section(&mut self, ui: &mut egui::Ui) {
        collapsible_section(
            ui,
            "display",
            "Display",
            Some(&[
                ("Navigation", ACCENT),
                (
                    "←/→ pans when zoomed, otherwise tunes RX. Hold to accelerate (2× then fast); Shift = fine, Ctrl = fast.",
                    MUTED,
                ),
                ("Minimap", ACCENT),
                (
                    "Top-right inset: CW band context + IQ data + viewport box. Click to pan (M).",
                    MUTED,
                ),
            ]),
            true,
            |ui| {
            let max_zoom = self.plot_max_zoom_out();
            scroll_slider_f32(ui, &mut self.plot_view.zoom, 0.04..=max_zoom, "View zoom");
            self.plot_view.clamp_pan(self.plot_full_span_hz(), max_zoom);
            let view = self.spectrum_view();
            let view_khz = view.view_span_hz / 1000.0;
            ui.label(
                egui::RichText::new(format!(
                    "Showing {view_khz:.1} kHz · zoom 1.0 = full IQ · {max_zoom:.1} = widest overview"
                ))
                .small()
                .color(MUTED),
            );
            ui.horizontal(|ui| {
                if ui.small_button("Full IQ (F)").clicked() {
                    self.plot_view.zoom_to_full_span();
                }
                if self.is_kiwi {
                    if ui.small_button("CW band view").clicked() {
                        let full_span = self.plot_full_span_hz();
                        let max_zoom = self.plot_max_zoom_out();
                        let segment = self.default_cw_segment_hz();
                        self.plot_view
                            .zoom_to_cw_segment(segment, full_span, max_zoom);
                    }
                }
            });
            ui.add_space(4.0);
            scroll_slider_f32_step(ui, &mut self.pan_step_hz, 50.0..=5000.0, "Pan step (Hz)", 50.0);
            scroll_slider_f32_step(
                ui,
                &mut self.pan_step_fast_hz,
                500.0..=50_000.0,
                "Fast pan step (Hz)",
                500.0,
            );
            self.pan_step_fast_hz = self.pan_step_fast_hz.max(self.pan_step_hz);
            if self.is_kiwi {
                toggle(
                    ui,
                    &mut self.show_band_overview,
                    "Band overview minimap (M)",
                );
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
                .on_hover_text(
                    "Keep adjusting Ref/Range as the band changes — RF gain will not change \
                     waterfall brightness while this is on",
                );
            });
            if scroll_slider_f32(ui, &mut self.ref_db, -120.0..=20.0, "Ref dB").changed() {
                self.display_levels_initialized = true;
                self.display_auto_track = false;
                self.force_texture_full = true;
                self.textures_dirty = true;
            }
            if scroll_slider_f32(ui, &mut self.range_db, 12.0..=80.0, "Range dB").changed() {
                self.display_levels_initialized = true;
                self.display_auto_track = false;
                self.force_texture_full = true;
                self.textures_dirty = true;
            }
            scroll_slider_f32(ui, &mut self.smooth_alpha, 0.05..=0.45, "Spectrum smooth");
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Waterfall avg").small().color(MUTED));
                for (label, n) in [("None", 1_u8), ("2×", 2), ("4×", 4)] {
                    if ui
                        .selectable_label(self.waterfall_avg == n, label)
                        .on_hover_text("Time-average consecutive FFT rows in the waterfall")
                        .clicked()
                    {
                        self.waterfall_avg = n;
                        self.force_texture_full = true;
                        self.textures_dirty = true;
                    }
                }
            });
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
        collapsible_section(ui, "perf", "Performance", None, false, |ui| {
            ui.checkbox(&mut self.fft_auto, "Auto FFT size (wideband)");
            ui.checkbox(
                &mut self.full_drain_spectrum,
                "Full-drain spectrum (wideband, more CPU)",
            )
            .on_hover_text(
                "FFT every drained IQ sample instead of the recent tail only. \
                 Row budget still adapts to CPU headroom.",
            );
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

            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Decim anti-alias").small().color(MUTED));
                if ui
                    .selectable_label(
                        self.cw.decim_filter == ChannelFilterKind::LinearFir,
                        "FIR",
                    )
                    .on_hover_text("Gaussian FIR before integer decimation (default)")
                    .clicked()
                {
                    self.cw.decim_filter = ChannelFilterKind::LinearFir;
                }
                if ui
                    .selectable_label(
                        self.cw.decim_filter == ChannelFilterKind::Iir2Pole,
                        "IIR 2-pole",
                    )
                    .on_hover_text("Biquad lowpass — ingress + channel decimator")
                    .clicked()
                {
                    self.cw.decim_filter = ChannelFilterKind::Iir2Pole;
                }
            });

            let mut fps = self.target_fps as f32;
            if scroll_slider_f32(ui, &mut fps, 10.0..=60.0, "Target FPS").changed() {
                self.target_fps = fps.round() as u32;
            }
            if self.is_wideband() && self.skimmer_enabled {
                ui.label(
                    egui::RichText::new(format!(
                        "Repaint capped at {} FPS while wideband + skimmer",
                        self.effective_target_fps()
                    ))
                    .small()
                    .color(MUTED),
                );
            }
            let eff_sk = self.effective_skimmer();
            if eff_sk.max_channels < self.skimmer.max_channels {
                ui.label(
                    egui::RichText::new(format!(
                        "Skimmer channels capped at {} on wideband",
                        eff_sk.max_channels
                    ))
                    .small()
                    .color(MUTED),
                );
            }

            ui.separator();
            stat_row(ui, "IQ / pump", self.stats.last_drain.to_string());
            stat_row(ui, "Decoders", self.skimmer_channels.to_string());
            if let Some(name) = &self.stats.audio_device {
                stat_row(ui, "Audio out", name.clone());
            }
        });
    }

    fn smeter_card(&mut self, ui: &mut egui::Ui) {
        let live = matches!(self.conn_state, ConnState::Streaming);
        section_frame()
            .inner_margin(egui::Margin::symmetric(8, 6))
            .show(ui, |ui| {
            let w = ui.available_width();
            ui.set_min_width(w);
            ui.set_max_width(w);
            section_heading_with_tip(
                ui,
                "S-meter",
                &[
                    ("RF level", ACCENT),
                    (
                        "Pre-software-AGC IQ + Kiwi hardware SND — independent of the IF IQ AGC loop.",
                        MUTED,
                    ),
                    ("IF IQ AGC", ACCENT),
                    (
                        "Software loop that holds AF steady — independent of the S-meter needle.",
                        MUTED,
                    ),
                    ("AF peak", OK),
                    ("Post-AGC audio level; aim near half scale when tuning RF gain.", MUTED),
                ],
            );
            show_dual_agc_loop(
                ui,
                &DualAgcParams {
                    rf_dbm: if live {
                        self.rf_meter_dbm()
                    } else {
                        -127.0
                    },
                    hw_rssi_dbm: if live {
                        self.stats.rssi_dbm
                    } else {
                        None
                    },
                    agc_gain: if live {
                        self.stats.agc_gain
                    } else {
                        1.0
                    },
                    agc_enabled: live && self.cw.agc.enabled,
                    audio_peak: if live {
                        self.stats.audio_peak
                    } else {
                        0.0
                    },
                    streaming: live,
                },
            );
        });
    }

    fn frequency_card(&mut self, ui: &mut egui::Ui) {
        section_card(ui, |ui| {
            section_heading(ui, "Operator");
            ui.label(egui::RichText::new("HF — all amateur bands 160m–10m").small().color(MUTED));
            self.band_preset_buttons(ui, &CW_HF_BAND_PRESETS);
            ui.label(egui::RichText::new("VHF+").small().color(MUTED));
            self.band_preset_buttons(ui, &CW_VHF_BAND_PRESETS);
            ui.horizontal(|ui| {
                let mut vfo_changed = false;
                ui.vertical(|ui| {
                    vfo_changed = vfo_wheel_khz(ui, &mut self.center_khz);
                });
                ui.with_layout(
                    egui::Layout::bottom_up(egui::Align::Min),
                    |ui| {
                        if band_lock_toggle(ui, &mut self.lock_ham_bands) {
                            if self.lock_ham_bands {
                                self.clamp_center_to_ham_bands();
                                vfo_changed = true;
                            }
                        }
                    },
                );
                if vfo_changed {
                    self.clamp_center_to_ham_bands();
                    self.apply_radio_settings();
                }
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                scroll_slider_f32_step(ui, &mut self.rit_hz, -800.0..=800.0, "RIT", 1.0);
                let rit_on = self.rit_hz.abs() > 0.5;
                if ui
                    .add_enabled(
                        rit_on,
                        egui::Button::new("Clear").min_size(egui::vec2(0.0, 0.0)),
                    )
                    .on_hover_text("Listen offset → 0 Hz without moving RX center (\\)")
                    .clicked()
                {
                    self.clear_rit();
                }
            });
        });
    }

    fn rf_front_end_card(&mut self, ui: &mut egui::Ui) {
        let live = matches!(self.conn_state, ConnState::Streaming);
        section_card(ui, |ui| {
            section_heading_with_tip(
                ui,
                "RF front-end",
                &[
                    ("RF gain", ACCENT),
                    (
                        "Raise until AF scope sits near half scale; lower if IQ AGC is pinned or AF clips.",
                        MUTED,
                    ),
                    ("Kiwi RF AGC", OK),
                    ("Turn off for manual RF gain — while on, Kiwi ignores the gain slider.", MUTED),
                ],
            );
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(self.connection_alias())
                        .small()
                        .color(MUTED),
                );
                if !live {
                    ui.label(
                        egui::RichText::new("offline — live on connect")
                            .small()
                            .color(MUTED),
                    );
                }
            });
            self.hardware_rf_controls(ui, live);
        });
    }

    fn af_tuning_card(&mut self, ui: &mut egui::Ui) {
        if !self.show_af_scope {
            return;
        }
        section_card(ui, |ui| {
            section_heading_with_tip(
                ui,
                "AF tuning",
                &[
                    ("Goal", ACCENT),
                    ("Tune RF gain so the AF trace sits near ±half scale.", MUTED),
                    ("Status badge", OK),
                    (
                        "LOW / OK / HOT reflects RF level, IQ AGC headroom, and AF peak.",
                        MUTED,
                    ),
                ],
            );
            let streaming = matches!(self.conn_state, ConnState::Streaming);
            let hint = classify_level(
                self.stats.audio_peak,
                self.cw.agc.enabled,
                self.stats.agc_gain,
                self.stats.agc_envelope,
                self.cw.agc.target,
                streaming,
            );
            af_scope::show_af_tuning_panel(
                ui,
                &AfScopeParams {
                    samples: &self.audio_scope,
                    peak: self.stats.audio_peak,
                    rms: self.stats.audio_rms,
                    agc_gain: self.stats.agc_gain,
                    agc_envelope: self.stats.agc_envelope,
                    agc_enabled: self.cw.agc.enabled,
                    agc_target: self.cw.agc.target,
                    iq_headroom: self.stats.iq_buffer_fill,
                    rssi_dbm: self.stats.rssi_dbm,
                    iq_rf_level: self.stats.iq_rf_level,
                    streaming,
                    hint,
                },
            );
        });
    }

    fn cw_carrier_tools(&mut self, ui: &mut egui::Ui) {
        let bfo = self.cw.bfo_hz.round();
        ui.horizontal(|ui| {
            if ui
                .button(format!("Zero-beat (Z) → {bfo:.0} Hz"))
                .on_hover_text(format!(
                    "Retune RX so the strongest carrier in view lands on your BFO ({bfo:.0} Hz audio tone); clears RIT"
                ))
                .clicked()
            {
                self.zero_beat();
            }
            toggle(
                ui,
                &mut self.pitch_lock,
                &format!("Lock pitch (L) @ {bfo:.0} Hz"),
            );
        });
    }

    fn cw_demod_card(&mut self, ui: &mut egui::Ui) {
        section_card(ui, |ui| {
            section_heading_with_tip(
                ui,
                "CW demod",
                &[
                    ("Channel filter", ACCENT),
                    (
                        "Complex IQ filter before demod — rejects adjacent signals while the carrier is still recoverable.",
                        MUTED,
                    ),
                    ("Plot", ACCENT),
                    (
                        "Ctrl+scroll: BW · drag cyan band = RIT · cyan edges = width · purple notches draggable.",
                        MUTED,
                    ),
                ],
            );
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new("BFO").small().color(MUTED));
                for (label, hz) in BFO_PRESETS {
                    if ui.selectable_label(self.cw.bfo_hz.round() == hz, label).clicked() {
                        self.cw.bfo_hz = hz;
                    }
                }
            });
            scroll_slider_f32_step(ui, &mut self.cw.bfo_hz, 300.0..=1_200.0, "BFO tone", 10.0);
            self.cw_carrier_tools(ui);
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
                "Channel filter",
            );
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Architecture").small().color(MUTED));
                if ui
                    .selectable_label(
                        self.cw.channel_filter == ChannelFilterKind::LinearFir,
                        "FIR (linear)",
                    )
                    .on_hover_text("Linear-phase windowed sinc — best CW keying, tunable shape")
                    .clicked()
                {
                    self.cw.channel_filter = ChannelFilterKind::LinearFir;
                }
                if ui
                    .selectable_label(
                        self.cw.channel_filter == ChannelFilterKind::Iir2Pole,
                        "IIR 2-pole",
                    )
                    .on_hover_text("Biquad lowpass — steeper skirts, may ring on edges (A/B)")
                    .clicked()
                {
                    self.cw.channel_filter = ChannelFilterKind::Iir2Pole;
                }
            });
            if self.cw.channel_filter == ChannelFilterKind::LinearFir {
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
                    window_choice(
                        ui,
                        &mut self.cw.window,
                        WindowKind::Kaiser,
                        "Kaiser",
                        "Tunable β — flat passband vs steep skirts (adjust β below)",
                    );
                });
                if self.cw.window == WindowKind::Kaiser {
                    scroll_slider_f32(ui, &mut self.cw.kaiser_beta, 2.0..=14.0, "Kaiser β");
                }
                let flatten_resp =
                    ui.checkbox(&mut self.cw.passband_flatten, "Flatten passband (inv-sinc)");
                attach_rich_tooltip(
                    &flatten_resp,
                    Some("Flatten passband"),
                    &[
                        ("Inv-sinc lift", ACCENT),
                        (
                            "Lifts upstream boxcar/CIC droop (N≈7). Off by default — enable if the tone sounds dull at band edges.",
                            MUTED,
                        ),
                    ],
                );
            }
            let audio_rate = hfsdr::audio_sample_rate(self.sample_rate, self.cw.decimation);
            let delay_note = if self.cw.channel_filter == ChannelFilterKind::LinearFir {
                let delay_ms = channel_group_delay_ms(audio_rate, self.cw.passband_hz);
                format!("Filter delay ~{delay_ms:.0} ms (linear-phase FIR)")
            } else {
                "IIR 2-pole — minimal delay, non-linear phase (may ring)".to_string()
            };
            ui.label(egui::RichText::new(delay_note).small().color(MUTED));
            self.agc_controls(ui);
        });
    }

    fn receive_chain_card(&mut self, ui: &mut egui::Ui) {
        collapsible_section(
            ui,
            "pipeline",
            "Receive chain",
            Some(&[
                ("Order", ACCENT),
                (
                    "Stages run top-to-bottom. Prefer IQ notches + channel filter before post-demod polish.",
                    MUTED,
                ),
                ("① IQ", OK),
                ("Noise blanker → manual notches (keys 1–4, ±80 Hz).", MUTED),
                ("②–④", OK),
                ("Channel filter + AGC + BFO in CW demod panel (right).", MUTED),
                ("⑤ Audio", ACCENT),
                ("APF, auto-notch, NR — optional post-demod stages.", MUTED),
            ]),
            true,
            |ui| {
            ui.label(egui::RichText::new("① IQ — before demod").small().color(MUTED));
            stage_toggle(
                ui,
                &mut self.cw.noise_blanker.enabled,
                "Noise blanker",
                Some("Wideband IQ impulse blanker"),
                Some("B"),
                Some(&[
                    ("Raw IQ", ACCENT),
                    (
                        "Blank lightning/ignition impulses — must run before the narrow channel filter.",
                        WARN,
                    ),
                ]),
            );
            if self.cw.noise_blanker.enabled {
                scroll_slider_f32(ui, &mut self.cw.noise_blanker.threshold, 2.0..=12.0, "NB threshold");
                let mut width = self.cw.noise_blanker.width as f32;
                scroll_slider_f32(ui, &mut width, 1.0..=30.0, "NB recovery");
                self.cw.noise_blanker.width = width.round() as usize;
            }

            ui.separator();
            self.manual_notches_body(ui);

            ui.separator();
            ui.label(egui::RichText::new("⑤ Audio — after BFO demod (optional)").small().color(MUTED));
            stage_toggle(
                ui,
                &mut self.cw.apf.enabled,
                "Audio peak filter",
                Some("Resonant boost at BFO pitch"),
                Some("P"),
                None,
            );
            if self.cw.apf.enabled {
                scroll_slider_f32(ui, &mut self.cw.apf.width_hz, 40.0..=300.0, "APF width");
                scroll_slider_f32(ui, &mut self.cw.apf.gain, 0.2..=4.0, "APF gain");
            }

            stage_toggle(
                ui,
                &mut self.cw.auto_notch.enabled,
                "Auto-notch",
                Some("Audio LMS with BFO guard"),
                Some("N"),
                Some(&[
                    ("Post-demod", ACCENT),
                    (
                        "Can see your BFO tone and freeze while you copy.",
                        MUTED,
                    ),
                    (
                        "Purple IQ notches above are better for hets — they run before demod.",
                        OK,
                    ),
                ]),
            );
            if self.cw.auto_notch.enabled {
                scroll_slider_f32(ui, &mut self.cw.auto_notch.guard_hz, 60.0..=300.0, "Guard ±Hz");
                scroll_slider_f32(ui, &mut self.cw.auto_notch.rate, 0.002..=0.1, "Adapt rate");
            }

            stage_toggle(
                ui,
                &mut self.cw.noise_reduction.enabled,
                "Noise reduction",
                Some("Light audio LMS polish"),
                Some("R"),
                Some(&[
                    ("Optional polish", ACCENT),
                    (
                        "The IQ channel filter is the real noise remover — NR does not belong before demod.",
                        MUTED,
                    ),
                ]),
            );
            if self.cw.noise_reduction.enabled {
                scroll_slider_f32(ui, &mut self.cw.noise_reduction.level, 0.0..=0.5, "NR level");
            }
        });
    }

    fn manual_notches_body(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let label = ui.label(
                egui::RichText::new("Manual notches — complex IQ")
                    .small()
                    .color(MUTED),
            );
            let hint = ui.label(egui::RichText::new("(?)").small().color(MUTED));
            let tip = &[
                ("Pre-demod", ACCENT),
                (
                    "Removes hets while the carrier is still recoverable. Drag purple markers on the spectrum.",
                    MUTED,
                ),
                ("Keys 1–4", OK),
                ("Toggle notches · new ones land on listen ±80 Hz.", MUTED),
            ];
            attach_rich_tooltip(&label, Some("Manual notches"), tip);
            attach_rich_tooltip(&hint, Some("Manual notches"), tip);
        });
        for idx in 0..MAX_NOTCHES {
            let was_enabled = self.cw.notches[idx].enabled;
            let key = match idx {
                0 => "1",
                1 => "2",
                2 => "3",
                _ => "4",
            };
            stage_toggle(
                ui,
                &mut self.cw.notches[idx].enabled,
                &format!("Manual notch #{}", idx + 1),
                Some("Complex IQ — drag on spectrum"),
                Some(key),
                None,
            );
            if self.cw.notches[idx].enabled && !was_enabled {
                self.arm_manual_notch(idx, None);
            }
            if self.cw.notches[idx].enabled {
                let notch = &mut self.cw.notches[idx];
                let mut offset_hz = notch.offset_hz.hz();
                scroll_slider_f32_step(
                    ui,
                    &mut offset_hz,
                    -5_000.0..=5_000.0,
                    "Offset",
                    1.0,
                );
                notch.offset_hz = ChannelOffsetHz::new(offset_hz);
                scroll_slider_f32_step(ui, &mut notch.width_hz, 10.0..=200.0, "Width", 1.0);
            }
        }
    }

    fn agc_controls(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.label(egui::RichText::new("④ Level — IQ envelope, before demod").small().color(MUTED));
        stage_toggle(
            ui,
            &mut self.cw.agc.enabled,
            "AGC",
            Some("IQ envelope gain riding"),
            Some("A"),
            None,
        );
        if self.cw.agc.enabled {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Mode").small().color(MUTED));
                if ui
                    .selectable_label(self.cw.agc_mode == AgcMode::Envelope, "Envelope")
                    .on_hover_text("Symmetric attack/decay — general-purpose; gain follows IQ level evenly")
                    .clicked()
                {
                    self.cw.agc_mode = AgcMode::Envelope;
                }
                if ui
                    .selectable_label(self.cw.agc_mode == AgcMode::Hang, "Hang")
                    .on_hover_text(
                        "Fast gain reduction, slow recovery — less noise lift between dits; \
                         most audible vs Envelope on weak CW with band noise",
                    )
                    .clicked()
                {
                    self.cw.agc_mode = AgcMode::Hang;
                }
                if ui
                    .selectable_label(self.cw.agc_mode == AgcMode::DualLoop, "Dual-loop")
                    .on_hover_text(
                        "Fast peak + slow floor trackers — resists pumping from strong neighbours; \
                         try when Envelope breathes on QRM",
                    )
                    .clicked()
                {
                    self.cw.agc_mode = AgcMode::DualLoop;
                }
            });
            scroll_slider_f32(ui, &mut self.cw.agc.attack_ms, 1.0..=20.0, "Attack ms");
            scroll_slider_f32(ui, &mut self.cw.agc.decay_ms, 20.0..=600.0, "Decay ms");
            scroll_slider_f32(ui, &mut self.cw.agc.target, 0.05..=0.6, "Target");
        } else {
            scroll_slider_f32(ui, &mut self.cw.agc.manual_gain, 0.1..=16.0, "Manual gain");
        }
    }

    fn hardware_rf_controls(&mut self, ui: &mut egui::Ui, live: bool) {
        match self.form_kind {
            SourceKind::Kiwi => self.kiwi_rf_controls(ui, live),
            #[cfg(feature = "airspy")]
            SourceKind::Airspy => self.airspy_rf_controls(ui, live),
            #[cfg(feature = "rtlsdr")]
            SourceKind::RtlSdr => self.rtlsdr_rf_controls(ui, live),
            #[cfg(feature = "qmx")]
            SourceKind::Qmx => self.qmx_rf_controls(ui, live),
        }
    }

    fn kiwi_rf_controls(&mut self, ui: &mut egui::Ui, live: bool) {
        if stage_toggle(
            ui,
            &mut self.agc_rf_on,
            "Kiwi RF AGC",
            Some("Hardware RF AGC on the Kiwi (CAT agc=)"),
            None,
            Some(&[
                ("Hardware loop", ACCENT),
                (
                    "When on, Kiwi runs its own SND AGC — the RF gain slider has no effect on IQ.",
                    MUTED,
                ),
                ("Dual AGC", OK),
                (
                    "Turn off for manual RF gain (Yaesu-style). Software IQ AGC is separate.",
                    MUTED,
                ),
            ]),
        ) {
            self.form_kiwi.rf_agc_on = self.agc_rf_on;
            self.sync_kiwi_rf_now();
        }
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("RF gain").small().color(MUTED));
            let mut gain_db = man_gain_db_below_max(self.form_kiwi.man_gain);
            let resp = ui.add(
                egui::Slider::new(&mut gain_db, -100..=0)
                    .suffix(" dB")
                    .clamping(egui::SliderClamping::Always),
            );
            if resp.changed() {
                self.form_kiwi.man_gain = man_gain_from_db_below_max(gain_db);
                self.sync_kiwi_rf_now();
            }
            if !self.agc_rf_on {
                ui.label(
                    egui::RichText::new("max")
                        .small()
                        .color(if gain_db == 0 { OK } else { MUTED }),
                );
            }
            if live {
                if let Some(hw) = self.stats.hw_rf_gain {
                    if hw == self.form_kiwi.man_gain {
                        ui.label(
                            egui::RichText::new("sent")
                                .small()
                                .color(OK),
                        );
                    }
                }
            }
            attach_rich_tooltip(
                &resp,
                Some("RF gain"),
                &[
                    ("Scale", ACCENT),
                    (
                        "0 dB = full gain (Kiwi manGain 100). Each step is ~1 dB; −50 dB is the old Kiwi default.",
                        MUTED,
                    ),
                    ("Kiwi RF AGC off", OK),
                    (
                        "Manual gain applies only with Kiwi RF AGC off — unlike a Yaesu, Kiwi IQ ignores manGain while AGC is on.",
                        MUTED,
                    ),
                    ("Yaesu analogy", MUTED),
                    (
                        "Start at 0 dB (max) and reduce gain if the band is hot — same idea as RF GAIN fully clockwise.",
                        MUTED,
                    ),
                ],
            );
        });
        if !live || self.stats.kiwi_has_rf_attn {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Attenuator").small().color(MUTED));
                let attn_live = live && self.stats.kiwi_has_rf_attn;
                ui.add_enabled_ui(attn_live || !live, |ui| {
                    ui.add(
                        egui::Slider::new(&mut self.form_kiwi.rf_attn_db, 0.0..=31.5)
                            .suffix(" dB")
                            .fixed_decimals(1),
                    );
                });
                if live && !self.stats.kiwi_has_rf_attn {
                    ui.label(
                        egui::RichText::new("(not on this Kiwi)")
                            .small()
                            .color(MUTED),
                    );
                } else if live {
                    ui.label(
                        egui::RichText::new(format!(
                            "hw {:.1} dB",
                            self.stats.kiwi_rf_attn_db
                        ))
                        .small()
                        .color(MUTED),
                    );
                }
            });
        }
    }

    #[cfg(feature = "qmx")]
    fn qmx_rf_controls(&mut self, ui: &mut egui::Ui, live: bool) {
        let _ = live;
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("RF gain").small().color(MUTED));
            ui.add(
                egui::Slider::new(&mut self.form_qmx.rf_gain_db, 0..=99)
                    .suffix(" dB")
                    .logarithmic(false),
            );
        });
        if !live {
            ui.label(
                egui::RichText::new("RF gain applies when connected")
                    .small()
                    .color(MUTED),
            );
        }
    }

    #[cfg(feature = "rtlsdr")]
    fn rtlsdr_rf_controls(&mut self, ui: &mut egui::Ui, live: bool) {
        let _ = live;
        stage_toggle(
            ui,
            &mut self.form_rtlsdr.rtl_agc,
            "RTL2832 AGC",
            Some("Internal digital AGC in the RTL2832"),
            None,
            None,
        );
        stage_toggle(
            ui,
            &mut self.form_rtlsdr.manual_gain,
            "Manual tuner gain",
            Some("Fixed RF gain from the tuner IC"),
            None,
            None,
        );
        if self.form_rtlsdr.manual_gain {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Gain").small().color(MUTED));
                ui.add(
                    egui::DragValue::new(&mut self.form_rtlsdr.tuner_gain_db10)
                        .range(0..=500)
                        .speed(0.5)
                        .suffix(" ×0.1 dB"),
                );
            });
        }
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("PPM").small().color(MUTED));
            ui.add(
                egui::DragValue::new(&mut self.form_rtlsdr.ppm)
                    .range(-200..=200)
                    .speed(0.1),
            );
        });
        stage_toggle(
            ui,
            &mut self.form_rtlsdr.bias_tee,
            "Bias tee",
            Some("GPIO bias for active antennas / upconverters"),
            None,
            None,
        );
    }

    #[cfg(feature = "airspy")]
    fn airspy_rf_controls(&mut self, ui: &mut egui::Ui, live: bool) {
        let _ = live;
        stage_toggle(
            ui,
            &mut self.form_airspy.hf_lna,
            "Preamp (+6 dB LNA)",
            Some("Enable for passive loop/wire antennas; off for max dynamic range"),
            None,
            None,
        );
        stage_toggle(
            ui,
            &mut self.form_airspy.hf_agc,
            "HF AGC",
            Some("Hardware AGC on the Airspy front end"),
            None,
            Some(&[
                ("HF AGC on", ACCENT),
                (
                    "Controls front-end gain — turn AGC off to use the 0–48 dB attenuator.",
                    MUTED,
                ),
            ]),
        );
        if self.form_airspy.hf_agc {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("AGC threshold").small().color(MUTED));
                ui.selectable_value(
                    &mut self.form_airspy.hf_agc_threshold_high,
                    false,
                    "Low",
                );
                ui.selectable_value(
                    &mut self.form_airspy.hf_agc_threshold_high,
                    true,
                    "High",
                );
            });
        }
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Attenuator").small().color(MUTED));
            ui.add_enabled(
                !self.form_airspy.hf_agc,
                egui::Slider::new(&mut self.form_airspy.hf_att, 0..=8)
                    .suffix(" ×6 dB"),
            );
        });
        stage_toggle(
            ui,
            &mut self.form_airspy.bias_tee,
            "Bias tee",
            Some("DC on antenna port for active preamps/upconverters"),
            None,
            None,
        );
        ui.collapsing("Frontend options (Discovery / Ranger)", |ui| {
            ui.toggle_value(
                &mut self.form_airspy.frontend_optimize_band_iii,
                "Optimize VHF Band III",
            );
            ui.toggle_value(
                &mut self.form_airspy.frontend_optimize_pll_boundary,
                "Optimize PLL integer boundary",
            );
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
                let bigram = ui.selectable_label(
                    self.skimmer.decoder == SkimmerDecoderKind::Bigram,
                    "Bigram beam",
                );
                attach_rich_tooltip(
                    &bigram,
                    Some("Decoder"),
                    &[
                        ("Bigram beam", ACCENT),
                        ("Best copy on pileups.", OK),
                        ("Adaptive", ACCENT),
                        ("Lighter CPU.", MUTED),
                    ],
                );
                if bigram.clicked() {
                    self.skimmer.decoder = SkimmerDecoderKind::Bigram;
                }
                let adaptive = ui.selectable_label(
                    self.skimmer.decoder == SkimmerDecoderKind::Adaptive,
                    "Adaptive",
                );
                attach_rich_tooltip(
                    &adaptive,
                    Some("Decoder"),
                    &[
                        ("Bigram beam", ACCENT),
                        ("Best copy on pileups.", OK),
                        ("Adaptive", ACCENT),
                        ("Lighter CPU.", MUTED),
                    ],
                );
                if adaptive.clicked() {
                    self.skimmer.decoder = SkimmerDecoderKind::Adaptive;
                }
            });
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
                            let call = match (spot.callsign.as_deref(), spot.kind) {
                                (Some(c), _) => c,
                                (None, SpotKind::CallingCq) => "CQ",
                                (None, _) => "…",
                            };
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
            stage_toggle(
                ui,
                &mut self.audio_enabled,
                "Speakers",
                Some("Spectrum/waterfall keep running when off"),
                Some("Space"),
                Some(&[
                    ("Mute", ACCENT),
                    (
                        "Muting speakers or volume 0 keeps spectrum, waterfall, and skimmer running.",
                        MUTED,
                    ),
                ]),
            );
            scroll_slider_f32(ui, &mut self.volume, 0.0..=4.0, "Volume (- / +)");
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
}

fn plot_action_changes_view(action: &PlotAction) -> bool {
    matches!(
        action,
        PlotAction::PanViewDeltaHz(_) | PlotAction::ZoomView(_) | PlotAction::SetViewPanHz(_)
    )
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
        WindowKind::Kaiser => 3,
    }
}

fn window_from_u8(v: u8) -> WindowKind {
    match v {
        1 => WindowKind::RaisedCosine,
        2 => WindowKind::Blackman,
        3 => WindowKind::Kaiser,
        _ => WindowKind::Gaussian,
    }
}

fn channel_filter_to_u8(k: ChannelFilterKind) -> u8 {
    match k {
        ChannelFilterKind::LinearFir => 0,
        ChannelFilterKind::Iir2Pole => 1,
    }
}

fn channel_filter_from_u8(v: u8) -> ChannelFilterKind {
    match v {
        1 => ChannelFilterKind::Iir2Pole,
        _ => ChannelFilterKind::LinearFir,
    }
}

fn agc_mode_to_u8(m: AgcMode) -> u8 {
    match m {
        AgcMode::Envelope => 0,
        AgcMode::Hang => 1,
        AgcMode::DualLoop => 2,
    }
}

fn agc_mode_from_u8(v: u8) -> AgcMode {
    match v {
        1 => AgcMode::Hang,
        2 => AgcMode::DualLoop,
        _ => AgcMode::Envelope,
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

fn all_source_kinds() -> Vec<SourceKind> {
    let mut kinds = vec![SourceKind::Kiwi];
    #[cfg(feature = "airspy")]
    kinds.push(SourceKind::Airspy);
    #[cfg(feature = "rtlsdr")]
    kinds.push(SourceKind::RtlSdr);
    #[cfg(feature = "qmx")]
    kinds.push(SourceKind::Qmx);
    kinds
}

fn source_kind_label(kind: SourceKind) -> &'static str {
    match kind {
        SourceKind::Kiwi => "KiwiSDR",
        #[cfg(feature = "airspy")]
        SourceKind::Airspy => "Airspy",
        #[cfg(feature = "rtlsdr")]
        SourceKind::RtlSdr => "RTL-SDR",
        #[cfg(feature = "qmx")]
        SourceKind::Qmx => "QMX",
    }
}

fn source_kind_labels() -> Vec<&'static str> {
    all_source_kinds()
        .into_iter()
        .map(source_kind_label)
        .collect()
}

fn source_kind_index(kind: SourceKind) -> usize {
    all_source_kinds()
        .iter()
        .position(|&k| k == kind)
        .unwrap_or(0)
}

fn source_kind_from_index(i: usize) -> SourceKind {
    all_source_kinds()
        .get(i)
        .copied()
        .unwrap_or(SourceKind::Kiwi)
}

fn is_local_source(kind: SourceKind) -> bool {
    match kind {
        SourceKind::Kiwi => false,
        #[cfg(feature = "airspy")]
        SourceKind::Airspy => true,
        #[cfg(feature = "rtlsdr")]
        SourceKind::RtlSdr => true,
        #[cfg(feature = "qmx")]
        SourceKind::Qmx => true,
    }
}

impl eframe::App for WaterfallApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if !self.themed {
            apply(&ctx);
            self.themed = true;
        }

        if let Some(mut req) = self.pending_connect.take() {
            self.form_kind = req.kind;
            self.form_host = req.host.clone();
            self.form_port = req.port;
            self.form_kiwi = req.kiwi.clone();
            if req.sample_rate != 0 {
                self.form_sample_rate = req.sample_rate;
            }
            self.form_airspy = req.airspy.clone();
            self.form_rtlsdr = req.rtlsdr.clone();
            self.form_qmx = req.qmx.clone();
            self.center_khz = req.center_hz / 1000.0;
            self.clamp_center_to_ham_bands();
            req.center_hz = self.center_khz * 1000.0;
            self.last_center_khz = self.center_khz;
            log::info(format!("connecting to {}", req.label()));
            self.engine.send(EngineCommand::Connect(req));
        }

        self.poll_scp_download();
        self.poll_kiwi_directory();
        self.handle_shortcuts(&ctx);
        self.pump_engine();
        self.frame_visible_spots = self.visible_spots();

        self.update_plot_hover(&ctx);
        egui::Panel::top("status")
            .frame(status_panel_frame())
            .show_inside(ui, |ui| self.status_banner(ui));

        if self.show_left || self.show_smeter {
            egui::Panel::left("left")
                .resizable(true)
                .frame(side_panel_frame())
                .size_range(LEFT_PANEL_MIN_W..=LEFT_PANEL_MAX_W)
                .default_size(if self.show_smeter && !self.show_left {
                    LEFT_PANEL_MIN_W
                } else {
                    300.0
                })
                .show_inside(ui, |ui| self.left_panel(ui));
        }

        if self.show_right {
            egui::Panel::right("controls")
                .resizable(true)
                .frame(side_panel_frame())
                .size_range(RIGHT_PANEL_MIN_W..=RIGHT_PANEL_MAX_W)
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

        self.latest_frame_tick = false;

        self.connection_popup(&ctx);
        self.iq_popup(&ctx);
        self.pipeline_popup(&ctx);
        self.shortcuts_popup(&ctx);

        self.apply_radio_settings();
        self.autosave();

        let frame_ms = (1000 / self.effective_target_fps().max(1)).max(8) as u64;
        ctx.request_repaint_after(Duration::from_millis(frame_ms));
    }

    fn on_exit(&mut self) {
        self.current_settings().save();
        self.engine.shutdown_now();
    }
}

fn normalize_waterfall_avg(value: u8) -> u8 {
    match value {
        2 => 2,
        4 => 4,
        _ => 1,
    }
}
