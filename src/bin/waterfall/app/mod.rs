//! Waterfall application state and rendering.
//!
//! The UI thread owns no DSP: it pushes settings to the [`crate::engine`] worker,
//! drains spectrum rows / status / spots it publishes, renders, and repaints
//! lazily. Connection lifecycle (connect, slow/unstable warnings, auto-reconnect)
//! is driven by the engine and surfaced here.

#![allow(unused_imports)]

mod codec;
mod constants;
mod methods;
mod state;

#[path = "prelude.rs"]
mod prelude;
pub(crate) use prelude::*;
pub(crate) use state::{
    AudioUiState, ChromeState, ConnectionFormState, ConnectionState, DisplayState, EngineUiState,
    KiwiDirectoryState, MeterDisplayState, FilterOverlayCache, PlotState, RadioState, SkimmerUiState,
    WaterfallTextureCache,
};

pub(crate) use constants::*;

pub struct WaterfallApp {
    engine: EngineHandle,
    pub(crate) engine_ui: EngineUiState,
    pub(crate) connection: ConnectionState,
    pub(crate) radio: RadioState,
    pub(crate) plot: PlotState,
    pub(crate) display: DisplayState,
    pub(crate) audio: AudioUiState,
    pub(crate) skimmer_ui: SkimmerUiState,
    pub(crate) chrome: ChromeState,
    pub(crate) meter_display: MeterDisplayState,
    resolver: ContinentResolver,
    annotated: HashSet<String>,
    slow: SlowWaterfall,
    last_settings_snapshot: Option<AppSettings>,
    settings_dirty_at: Option<std::time::Instant>,
}

impl WaterfallApp {
    pub fn new(autoconnect: Option<ConnectRequest>) -> Self {
        Self::build(autoconnect, EngineHandle::spawn())
    }

    /// Headless UI tests: no engine thread; feed [`EnginePoll`] via [`Self::inject_engine_poll`].
    #[cfg(test)]
    pub fn new_for_test(autoconnect: Option<ConnectRequest>) -> Self {
        let mut app = Self::build(autoconnect, EngineHandle::spawn_for_test());
        // Deterministic defaults — do not inherit the developer's on-disk settings.
        app.apply_settings(&AppSettings::default());
        app.last_settings_snapshot = Some(app.current_settings());
        app
    }

    #[cfg(test)]
    pub fn inject_engine_poll(&self, poll: EnginePoll) {
        self.engine.inject_poll(poll);
    }

    #[cfg(test)]
    pub fn history_labels(&self) -> Vec<String> {
        self.slow
            .annotations()
            .iter()
            .map(|a| a.label.clone())
            .collect()
    }

    fn build(autoconnect: Option<ConnectRequest>, engine: EngineHandle) -> Self {
        let saved = AppSettings::load();
        let audio_devices = AudioOutput::list_output_devices();

        let mut app = Self {
            engine,
            engine_ui: EngineUiState::default(),
            connection: ConnectionState {
                form: ConnectionFormState {
                    pending_connect: None,
                    kind: SourceKind::Kiwi,
                    host: String::new(),
                    port: 8073,
                    kiwi: KiwiSettings::default(),
                    sample_rate: 384_000,
                    airspy: AirspySettings::default(),
                    last_airspy_rf: AirspySettings::default(),
                    rtlsdr: RtlSdrSettings::default(),
                    last_rtlsdr_rf: RtlSdrSettings::default(),
                    qmx: QmxSettings::default(),
                    last_qmx_rf: QmxSettings::default(),
                    recent_hosts: Vec::new(),
                    show_connection_drawer: false,
                },
                kiwi: KiwiDirectoryState {
                    geo: None,
                    nearby: Vec::new(),
                    fetch_rx: None,
                    error: None,
                },
            },
            radio: RadioState {
                sample_rate: 12_000.0,
                center_khz: DEFAULT_CENTER_HZ / 1000.0,
                last_center_khz: DEFAULT_CENTER_HZ / 1000.0,
                is_kiwi: false,
                cw: CwChannelSettings::default(),
                rit_hz: 0.0,
                rit_on: false,
                pitch_lock: false,
                lock_ham_bands: true,
                agc_rf_on: true,
                last_agc_rf_on: true,
                rf_gain_db: 0.0,
                last_kiwi_man_gain: hfsdr::kiwi::protocol::KIWI_MAN_GAIN_DEFAULT,
                last_kiwi_rf_attn_db: 0.0,
                last_kiwi_has_rf_attn: false,
                last_snr_db: 0.0,
                passband_wide: false,
            },
            plot: PlotState {
                rows: VecDeque::with_capacity(WATERFALL_ROWS),
                latest: vec![-120.0; FFT_SIZE],
                smoothed_trace: Vec::new(),
                trace_composed: Vec::new(),
                trace_view_key: TraceViewKey::new(0.0, 0.0, 0.0, 0.0, 0),
                overview_smoothed: Vec::new(),
                overview_composed: Vec::new(),
                overview_view_key: TraceViewKey::new(0.0, 0.0, 0.0, 0.0, 0),
                latest_frame_tick: false,
                waterfall: WaterfallTextureCache {
                    textures_dirty: false,
                    force_texture_full: true,
                    ..WaterfallTextureCache::default()
                },
                last_display_levels_at: None,
                panadapter_plot: PanadapterPlot::new(),
                plot_view: PlotViewState::new(),
                plot_interaction: PlotInteraction::new(),
                hover_offset_hz: None,
                last_plot_interaction_rect: None,
                filter_overlay: FilterOverlayCache::default(),
                tune_preview_offset_hz: None,
            },
            display: DisplayState {
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
                perf_trace: false,
            },
            audio: AudioUiState {
                audio_devices,
                selected_audio_device: 0,
                last_audio_device: 0,
                audio_enabled: true,
                volume: 1.0,
                audio_scope: Vec::new(),
            },
            skimmer_ui: SkimmerUiState {
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
                frame_visible_spots: Vec::new(),
            },
            chrome: ChromeState {
                show_console: false,
                show_shortcuts: false,
                show_af_scope: true,
                show_smeter: true,
                cw_simple_ui: true,
                show_history: false,
                show_left: true,
                show_right: true,
                show_iq_drawer: false,
                show_pipeline_drawer: false,
                show_filter_drawer: false,
                pipeline_flow: PipelineFlow::new(),
                filter_diagnostic: crate::filter_diagnostic::FilterDiagnosticState::default(),
                notch_bypass_stash: None,
                iq: IqPanel::new(hfsdr::default_capture_dir()),
                themed: false,
            },
            meter_display: MeterDisplayState::default(),
            resolver: ContinentResolver::new(),
            annotated: HashSet::new(),
            slow: SlowWaterfall::new(2.0, 600.0, RowFold::Peak),
            last_settings_snapshot: None,
            settings_dirty_at: None,
        };

        app.apply_settings(&saved);

        if let Some(r) = app.connection.form.recent_hosts.first().cloned() {
            app.apply_connect_form(&r);
        }

        if let Some(req) = autoconnect {
            app.connection.form.kind = req.kind;
            app.connection.form.host = req.host.clone();
            app.connection.form.port = req.port;
            app.connection.form.kiwi = req.kiwi.clone();
            if req.sample_rate != 0 {
                app.connection.form.sample_rate = req.sample_rate;
            }
            app.connection.form.airspy = req.airspy.clone();
            app.connection.form.rtlsdr = req.rtlsdr.clone();
            app.connection.form.qmx = req.qmx.clone();
            app.radio.center_khz = req.center_hz / 1000.0;
            app.clamp_center_to_ham_bands();
            app.radio.last_center_khz = app.radio.center_khz;
            app.connection.form.pending_connect = Some(req);
            app.connection.form.show_connection_drawer = false;
        }

        app.last_settings_snapshot = Some(app.current_settings());
        if let Some((geo, receivers)) = crate::kiwi_directory::load_cached_receivers() {
            app.connection.kiwi.geo = geo;
            app.connection.kiwi.nearby = receivers;
        }
        #[cfg(not(test))]
        app.start_kiwi_directory_fetch(false);
        app.apply_default_view_zoom();
        app
    }
}

impl eframe::App for WaterfallApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if !self.chrome.themed {
            apply(&ctx);
            self.chrome.themed = true;
        }

        if let Some(mut req) = self.connection.form.pending_connect.take() {
            self.connection.form.kind = req.kind;
            self.connection.form.host = req.host.clone();
            self.connection.form.port = req.port;
            self.connection.form.kiwi = req.kiwi.clone();
            if req.sample_rate != 0 {
                self.connection.form.sample_rate = req.sample_rate;
            }
            self.connection.form.airspy = req.airspy.clone();
            self.connection.form.rtlsdr = req.rtlsdr.clone();
            self.connection.form.qmx = req.qmx.clone();
            self.radio.center_khz = req.center_hz / 1000.0;
            self.clamp_center_to_ham_bands();
            req.center_hz = self.radio.center_khz * 1000.0;
            self.radio.last_center_khz = self.radio.center_khz;
            log::info(format!("connecting to {}", req.label()));
            self.engine.send(EngineCommand::Connect(req));
        }

        self.poll_scp_download();
        self.poll_kiwi_directory();
        self.handle_shortcuts(&ctx);
        self.pump_engine();
        self.skimmer_ui.frame_visible_spots = self.visible_spots();

        let meter_dt = ui.input(|i| i.stable_dt);
        self.tick_meter_display(meter_dt);

        self.update_plot_hover(&ctx);
        egui::Panel::top("status")
            .frame(status_panel_frame())
            .show_inside(ui, |ui| self.status_banner(ui));

        if self.chrome.show_left || self.chrome.show_smeter {
            egui::Panel::left("left")
                .resizable(true)
                .frame(side_panel_frame())
                .size_range(LEFT_PANEL_MIN_W..=LEFT_PANEL_MAX_W)
                .default_size(if self.chrome.show_smeter && !self.chrome.show_left {
                    LEFT_PANEL_MIN_W
                } else {
                    300.0
                })
                .show_inside(ui, |ui| self.left_panel(ui));
        }

        if self.chrome.show_right {
            egui::Panel::right("controls")
                .resizable(true)
                .frame(side_panel_frame())
                .size_range(RIGHT_PANEL_MIN_W..=RIGHT_PANEL_MAX_W)
                .default_size(280.0)
                .show_inside(ui, |ui| self.right_panel(ui));
        }

        if self.chrome.show_history {
            egui::Panel::bottom("history")
                .resizable(true)
                .default_size(150.0)
                .show_inside(ui, |ui| self.history_panel(ui));
        }

        if self.chrome.show_console {
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

        self.plot.latest_frame_tick = false;

        self.connection_popup(&ctx);
        self.iq_popup(&ctx);
        self.pipeline_popup(&ctx);
        self.filter_popup(&ctx);
        if self.chrome.show_filter_drawer {
            ctx.request_repaint();
        }
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
