//! Waterfall application state and rendering.
//!
//! The UI thread owns no DSP: it pushes settings to the [`crate::engine`] worker,
//! drains spectrum rows / status / spots it publishes, renders, and repaints
//! lazily. Connection lifecycle (connect, slow/unstable warnings, auto-reconnect)
//! is driven by the engine and surfaced here.

#![allow(unused_imports)]

mod codec;
mod constants;

pub(crate) use constants::*;

include!("prelude.rs");

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
}

include!(concat!(env!("OUT_DIR"), "/waterfall_impl_methods.inc"));

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
