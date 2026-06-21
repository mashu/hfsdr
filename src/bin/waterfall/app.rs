//! Waterfall application state and rendering.

use std::collections::VecDeque;

use std::collections::HashSet;

use eframe::egui;
use egui::Color32;
use hfsdr::{
    extract_view_window, strongest_offset_hz, Complex32, Consumer, Continent, ContinentResolver,
    CwChannelSettings, IqAudioDemod, IqSource, RowFold, SlowWaterfall, Spot, SpotKind, SpotSort,
    SpectrumAnalyzer, WindowKind, MAX_NOTCHES,
};

use crate::audio::AudioOutput;
use crate::colormap::db_to_colour;
use crate::controls::{scroll_drag_f64, scroll_slider_f32, scroll_slider_log_f32};
use crate::display_levels::estimate_levels;
use crate::interaction::{PlotAction, PlotInteraction, PlotViewState};
use crate::interaction::{CW_PASSBAND_MAX_HZ, CW_PASSBAND_MIN_HZ};
use crate::skimmer::SkimmerHandle;
use crate::theme::{
    apply, badge, collapsible_section, section_card, section_heading, section_hint, stat_row, MUTED,
    OK, WARN,
};
use crate::widgets::{display_trace, SpectrumWidget, SpotLabel, WaterfallWidget};

pub const FFT_SIZE: usize = 2048;
pub const WATERFALL_ROWS: usize = 360;
pub const MAX_DRAIN_PER_FRAME: usize = 300_000;
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
    source: Box<dyn IqSource>,
    iq: Consumer<Complex32>,
    analyzer: SpectrumAnalyzer,
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

    ref_db: f32,
    range_db: f32,
    display_levels_initialized: bool,
    smooth_alpha: f32,
    drain_scratch: Vec<Complex32>,
    audio_scratch: Vec<f32>,
    iq_per_frame: usize,
    waterfall_rows: usize,

    audio: Option<AudioOutput>,
    audio_devices: Vec<String>,
    selected_audio_device: usize,
    last_audio_device: usize,
    audio_demod: IqAudioDemod,
    audio_enabled: bool,
    volume: f32,

    skimmer: SkimmerHandle,
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

    spectrum_widget: SpectrumWidget,
    waterfall_widget: WaterfallWidget,
    plot_view: PlotViewState,
    plot_interaction: PlotInteraction,
    hover_offset_hz: Option<f64>,
    tune_preview_offset_hz: Option<f64>,
    themed: bool,
}

impl WaterfallApp {
    pub fn new(
        source: Box<dyn IqSource>,
        iq: Consumer<Complex32>,
        sample_rate: f32,
        center_hz: f64,
        is_kiwi: bool,
    ) -> Self {
        let source_rate = sample_rate as u32;
        let audio_devices = AudioOutput::list_output_devices();
        let selected_audio_device = 0usize;
        let audio = if audio_devices.is_empty() {
            AudioOutput::try_open_default(source_rate)
        } else {
            AudioOutput::try_open_named(&audio_devices[0], source_rate)
                .or_else(|| AudioOutput::try_open_default(source_rate))
        };

        let cw = CwChannelSettings {
            passband_hz: if is_kiwi { 200.0 } else { 250.0 },
            ..CwChannelSettings::default()
        };

        let skimmer = SkimmerHandle::spawn(if is_kiwi { "kiwi".into() } else { "airspy".into() });

        Self {
            source,
            iq,
            analyzer: SpectrumAnalyzer::new(FFT_SIZE, FFT_SIZE / 2),
            sample_rate,
            center_khz: center_hz / 1000.0,
            last_center_khz: center_hz / 1000.0,
            is_kiwi,
            cw,
            rit_hz: 0.0,
            pitch_lock: false,
            agc_rf_on: true,
            last_agc_rf_on: true,
            last_snr_db: 0.0,
            rows: VecDeque::with_capacity(WATERFALL_ROWS),
            latest: vec![-120.0; FFT_SIZE],
            smoothed_trace: Vec::new(),
            texture: None,
            ref_db: -70.0,
            range_db: 55.0,
            display_levels_initialized: false,
            smooth_alpha: SMOOTH_ALPHA,
            drain_scratch: Vec::with_capacity(MAX_DRAIN_PER_FRAME),
            audio_scratch: Vec::new(),
            iq_per_frame: 0,
            waterfall_rows: 0,
            audio,
            audio_devices,
            selected_audio_device,
            last_audio_device: selected_audio_device,
            audio_demod: IqAudioDemod::new(),
            audio_enabled: true,
            volume: 1.0,
            skimmer,
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
            spectrum_widget: SpectrumWidget::new(),
            waterfall_widget: WaterfallWidget::new(),
            plot_view: PlotViewState::new(),
            plot_interaction: PlotInteraction::new(),
            hover_offset_hz: None,
            tune_preview_offset_hz: None,
            themed: false,
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

    fn ingest(&mut self) {
        self.drain_scratch.clear();
        while self.drain_scratch.len() < MAX_DRAIN_PER_FRAME {
            match self.iq.pop() {
                Ok(s) => self.drain_scratch.push(s),
                Err(_) => break,
            }
        }
        self.iq_per_frame = self.drain_scratch.len();
        if self.drain_scratch.is_empty() {
            return;
        }

        if self.audio_enabled {
            self.cw.listen_offset_hz = self.listen_offset_hz() as f32;
            self.audio_demod.process(
                &self.drain_scratch,
                self.sample_rate,
                &self.cw,
                &mut self.audio_scratch,
            );
            self.last_snr_db = self.audio_demod.snr_db();
            if let Some(audio) = &mut self.audio {
                audio.push(&self.audio_scratch, self.sample_rate as u32, self.volume);
            }
        }

        let rows = &mut self.rows;
        let latest = &mut self.latest;
        let slow = &mut self.slow;
        self.analyzer.process(&self.drain_scratch, |row| {
            latest.copy_from_slice(row);
            slow.push_row(row);
            let mut stored = if rows.len() >= WATERFALL_ROWS {
                rows.pop_back().unwrap_or_else(|| vec![0.0; FFT_SIZE])
            } else {
                vec![0.0; FFT_SIZE]
            };
            stored.copy_from_slice(row);
            rows.push_front(stored);
        });
        self.waterfall_rows = rows.len();
        self.maybe_init_display_levels();
        self.apply_pitch_lock();

        if self.skimmer.enabled() {
            let center_hz = self.center_khz * 1000.0;
            self.skimmer
                .submit(&self.drain_scratch, &self.latest, self.sample_rate, center_hz);
            self.skimmer_spots = self.skimmer.spots();
            self.annotate_new_cqs(center_hz);
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
        let source_rate = self.sample_rate as u32;
        self.audio = self
            .audio_devices
            .get(self.selected_audio_device)
            .and_then(|name| AudioOutput::try_open_named(name, source_rate))
            .or_else(|| AudioOutput::try_open_default(source_rate));
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
        if self.is_kiwi && !self.source.link_ready() {
            return;
        }
        if (self.center_khz - self.last_center_khz).abs() > f64::EPSILON {
            let _ = self.source.tune(self.center_khz * 1000.0);
            self.last_center_khz = self.center_khz;
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let (
            zero, lock, notch, blank, nr, agc, narrow, widen, rit_dn, rit_up, full, mute,
        ) = ctx.input(|i| {
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
                *none = Some(ctx.load_texture("slow_waterfall", image, egui::TextureOptions::NEAREST));
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

    fn controls_panel(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                self.receiver_card(ui);
                self.frequency_card(ui);
                self.cw_demod_card(ui);
                self.filter_pipeline_card(ui);
                self.notch_card(ui);
                self.skimmer_card(ui);
                self.audio_card(ui);

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

                collapsible_section(ui, "debug", "Diagnostics", false, |ui| {
                    stat_row(ui, "IQ / frame", self.iq_per_frame.to_string());
                    stat_row(ui, "Dropped", self.source.dropped_samples().to_string());
                    if let Some(rssi) = self.source.rssi_dbm() {
                        stat_row(ui, "S-meter", format!("{:.1} dBm", rssi));
                    }
                });

                ui.add_space(4.0);
                section_hint(
                    ui,
                    "Keys: Z zero-beat · L pitch-lock · N notch · B NB · R NR · A AGC · [ ] width · , . RIT · F full · Space mute",
                );
            });

        if self.is_kiwi && self.agc_rf_on != self.last_agc_rf_on {
            let _ = self.source.set_agc(self.agc_rf_on);
            self.last_agc_rf_on = self.agc_rf_on;
        }

        self.apply_audio_device();
    }

    fn receiver_card(&mut self, ui: &mut egui::Ui) {
        section_card(ui, |ui| {
            section_heading(ui, "Receiver");
            stat_row(ui, "RX center", format!("{:.3} MHz", self.center_khz / 1000.0));
            stat_row(ui, "Listen", format!("{:.0} Hz", self.listen_offset_hz()));
            stat_row(ui, "SNR", format!("{:.0} dB", self.last_snr_db));
            stat_row(ui, "IQ", format!("{:.1} kS/s", self.sample_rate / 1000.0));
            if self.is_kiwi {
                ui.add_space(4.0);
                if let Some(err) = self.source.link_error() {
                    badge(ui, &err, Color32::RED);
                } else if !self.source.link_ready() {
                    badge(ui, "Kiwi: connecting…", WARN);
                } else {
                    badge(ui, "Kiwi: streaming", OK);
                }
            }
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
                    ui.label(egui::RichText::new(format!("Cursor {:.0} Hz", hover)).small().color(MUTED));
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
        let mut enabled = self.skimmer.enabled();
        section_card(ui, |ui| {
            section_heading(ui, "Skimmer");
            if ui.checkbox(&mut enabled, "Decode the whole span").changed() {
                self.skimmer.set_enabled(enabled);
            }
            ui.checkbox(&mut self.show_history, "Show band history");
            if enabled {
                stat_row(ui, "Decoders", self.skimmer.active_channels().to_string());
                stat_row(ui, "Spots", self.skimmer_spots.len().to_string());
            }
        });

        if !enabled {
            return;
        }

        collapsible_section(ui, "spots", "Spot table", true, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new("Sort").small().color(MUTED));
                sort_choice(ui, &mut self.spot_sort, SpotSort::SnrDesc, "SNR");
                sort_choice(ui, &mut self.spot_sort, SpotSort::Frequency, "Freq");
                sort_choice(ui, &mut self.spot_sort, SpotSort::LastHeard, "Time");
                sort_choice(ui, &mut self.spot_sort, SpotSort::Callsign, "Call");
            });
            scroll_slider_f32(ui, &mut self.min_spot_snr, 0.0..=40.0, "Min SNR");
            ui.checkbox(&mut self.continent_filter, "Filter by continent");
            if self.continent_filter {
                ui.horizontal_wrapped(|ui| {
                    for c in Continent::ALL {
                        let idx = continent_index(c);
                        let mut on = self.show_continents[idx];
                        if ui.selectable_label(on, c.code()).clicked() {
                            on = !on;
                            self.show_continents[idx] = on;
                        }
                    }
                });
            }

            ui.separator();
            let mut tune_to: Option<f64> = None;
            egui::ScrollArea::vertical()
                .max_height(280.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for spot in self.visible_spots() {
                        if spot_row(ui, &spot).clicked() {
                            tune_to = Some(spot.frequency_hz);
                        }
                    }
                });
            if let Some(hz) = tune_to {
                self.tune_to_hz(hz);
            }
        });
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
            if self.audio.is_some() {
                ui.checkbox(&mut self.audio_enabled, "Play on speakers (Space)");
                scroll_slider_f32(ui, &mut self.volume, 0.0..=4.0, "Volume");
                if let Some(audio) = &self.audio {
                    stat_row(ui, "Active", audio.device_name());
                    stat_row(ui, "Rate", format!("{} Hz", audio.output_rate()));
                }
            } else {
                ui.colored_label(WARN, "Could not open output device");
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
        let labels = if self.skimmer.enabled() {
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

fn sort_choice(ui: &mut egui::Ui, current: &mut SpotSort, kind: SpotSort, label: &str) {
    if ui.selectable_label(*current == kind, label).clicked() {
        *current = kind;
    }
}

fn continent_index(c: Continent) -> usize {
    Continent::ALL.iter().position(|&x| x == c).unwrap_or(0)
}

fn spot_row(ui: &mut egui::Ui, spot: &Spot) -> egui::Response {
    let glyph = match spot.kind {
        SpotKind::CallingCq => "CQ",
        SpotKind::Answering => "→",
        SpotKind::Heard => "·",
    };
    let call = spot.callsign.as_deref().unwrap_or("…");
    let color = if spot.kind == SpotKind::CallingCq { WARN } else { OK };
    let text = format!(
        "{glyph:<2} {call:<8} {:>8.1} kHz  {:>2.0}dB {:>2.0}wpm",
        spot.frequency_hz / 1000.0,
        spot.snr_db,
        spot.wpm,
    );
    ui.add(egui::Label::new(egui::RichText::new(text).monospace().color(color)).sense(egui::Sense::click()))
}

impl eframe::App for WaterfallApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if !self.themed {
            apply(&ctx);
            self.themed = true;
        }

        self.handle_shortcuts(&ctx);
        self.ingest();
        self.update_texture(&ctx);
        if self.show_history {
            self.update_history_texture(&ctx);
        }

        egui::Panel::right("controls")
            .resizable(true)
            .default_size(360.0)
            .show_inside(ui, |ui| self.controls_panel(ui));

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

        ctx.request_repaint();
    }
}
