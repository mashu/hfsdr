//! Waterfall application state and rendering.

use std::collections::VecDeque;

use eframe::egui;
use egui::Color32;
use hfsdr::{extract_view_window, Complex32, Consumer, DemodSettings, IqAudioDemod, IqSource, SpectrumAnalyzer};

use crate::audio::AudioOutput;
use crate::colormap::db_to_colour;
use crate::display_levels::estimate_levels;
use crate::interaction::{PlotAction, PlotInteraction, PlotViewState};
use crate::controls::{scroll_drag_f64, scroll_slider_log_f32, scroll_slider_f32};
use crate::interaction::{CW_PASSBAND_MAX_HZ, CW_PASSBAND_MIN_HZ};
use crate::theme::{
    apply, badge, collapsible_section, section_card, section_heading, section_hint, stat_row, MUTED, OK,
    WARN,
};
use crate::widgets::{display_trace, SpectrumWidget, WaterfallWidget};

pub const FFT_SIZE: usize = 2048;
pub const WATERFALL_ROWS: usize = 360;
pub const MAX_DRAIN_PER_FRAME: usize = 300_000;
const SMOOTH_ALPHA: f32 = 0.09;

const CW_PRESETS: [(&str, f64); 6] = [
    ("40m", 7_030_000.0),
    ("30m", 10_125_000.0),
    ("20m", 14_070_000.0),
    ("17m", 18_095_000.0),
    ("15m", 21_070_000.0),
    ("12m", 24_920_000.0),
];

const BFO_PRESETS: [(&str, f32); 5] = [
    ("500", 500.0),
    ("600", 600.0),
    ("650", 650.0),
    ("750", 750.0),
    ("800", 800.0),
];

pub struct WaterfallApp {
    source: Box<dyn IqSource>,
    iq: Consumer<Complex32>,
    analyzer: SpectrumAnalyzer,
    sample_rate: f32,
    center_khz: f64,
    last_center_khz: f64,
    is_kiwi: bool,
    passband_hz: f32,
    bfo_hz: f32,
    rit_hz: f32,
    xit_hz: f32,
    notch_enabled: bool,
    notch_offset_hz: f32,
    notch_width_hz: f32,
    squelch: f32,
    software_agc: bool,
    agc_on: bool,
    last_agc_on: bool,

    rows: VecDeque<Vec<f32>>,
    latest: Vec<f32>,
    smoothed_trace: Vec<f32>,
    texture: Option<egui::TextureHandle>,

    ref_db: f32,
    range_db: f32,
    display_levels_initialized: bool,
    smooth_alpha: f32,
    drain_scratch: Vec<Complex32>,
    iq_per_frame: usize,
    waterfall_rows: usize,

    audio: Option<AudioOutput>,
    audio_devices: Vec<String>,
    selected_audio_device: usize,
    last_audio_device: usize,
    audio_demod: IqAudioDemod,
    audio_enabled: bool,
    volume: f32,

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
        let passband_hz = if is_kiwi { 200.0 } else { 250.0 };
        let source_rate = sample_rate as u32;
        let audio_devices = AudioOutput::list_output_devices();
        let selected_audio_device = 0usize;
        let audio = if audio_devices.is_empty() {
            AudioOutput::try_open_default(source_rate)
        } else {
            AudioOutput::try_open_named(&audio_devices[0], source_rate)
                .or_else(|| AudioOutput::try_open_default(source_rate))
        };
        Self {
            source,
            iq,
            analyzer: SpectrumAnalyzer::new(FFT_SIZE, FFT_SIZE / 2),
            sample_rate,
            center_khz: center_hz / 1000.0,
            last_center_khz: center_hz / 1000.0,
            is_kiwi,
            passband_hz,
            bfo_hz: 650.0,
            rit_hz: 0.0,
            xit_hz: 0.0,
            notch_enabled: false,
            notch_offset_hz: 0.0,
            notch_width_hz: 40.0,
            squelch: 0.0,
            software_agc: true,
            agc_on: true,
            last_agc_on: true,
            rows: VecDeque::with_capacity(WATERFALL_ROWS),
            latest: vec![-120.0; FFT_SIZE],
            smoothed_trace: Vec::new(),
            texture: None,
            ref_db: -70.0,
            range_db: 55.0,
            display_levels_initialized: false,
            smooth_alpha: SMOOTH_ALPHA,
            drain_scratch: Vec::with_capacity(MAX_DRAIN_PER_FRAME),
            iq_per_frame: 0,
            waterfall_rows: 0,
            audio,
            audio_devices,
            selected_audio_device,
            last_audio_device: selected_audio_device,
            audio_demod: IqAudioDemod::new(),
            audio_enabled: true,
            volume: 1.0,
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
                PlotAction::TuneDeltaHz(delta) => {
                    self.center_khz += delta / 1000.0;
                    self.plot_view.pan_offset_hz = 0.0;
                    self.tune_preview_offset_hz = None;
                }
                PlotAction::CenterOnOffsetHz(offset) => {
                    self.center_khz += offset / 1000.0;
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
                    self.passband_hz = bw.clamp(CW_PASSBAND_MIN_HZ, CW_PASSBAND_MAX_HZ);
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
            let listen_offset = self.listen_offset_hz();
            let settings = DemodSettings {
                listen_offset_hz: listen_offset,
                bfo_hz: self.bfo_hz,
                passband_hz: self.passband_hz,
                notch_enabled: self.notch_enabled,
                notch_offset_hz: self.notch_offset_hz,
                notch_width_hz: self.notch_width_hz,
                squelch: self.squelch,
                software_agc: self.software_agc,
            };
            let mono = self.audio_demod.process(
                &self.drain_scratch,
                self.sample_rate,
                &settings,
            );
            if let Some(audio) = &mut self.audio {
                audio.push(&mono, self.sample_rate as u32, self.volume);
            }
        }

        let rows = &mut self.rows;
        let latest = &mut self.latest;
        self.analyzer.process(&self.drain_scratch, |row| {
            latest.copy_from_slice(row);
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

    fn listen_offset_hz(&self) -> f32 {
        self.rit_hz + self.tune_preview_offset_hz.unwrap_or(0.0) as f32
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

    fn controls_panel(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {

                section_card(ui, |ui| {
                    section_heading(ui, "Receiver");
                    stat_row(ui, "RX center", format!("{:.3} MHz", self.center_khz / 1000.0));
                    stat_row(ui, "Listen", format!("{:.0} Hz offset", self.listen_offset_hz()));
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

                section_card(ui, |ui| {
                    section_heading(ui, "Frequency");
                    ui.horizontal_wrapped(|ui| {
                        for (label, hz) in CW_PRESETS {
                            let selected = (self.center_khz * 1000.0).round() == hz;
                            if ui.selectable_label(selected, label).clicked() {
                                self.center_khz = hz / 1000.0;
                            }
                        }
                    });
                    scroll_drag_f64(
                        ui,
                        &mut self.center_khz,
                        0.0..=30_000.0,
                        0.05,
                        " kHz",
                    );
                    let prev_rit = self.rit_hz;
                    scroll_slider_f32(ui, &mut self.rit_hz, -800.0..=800.0, "RIT");
                    if self.rit_hz != prev_rit {
                        // RIT only moves listen passband.
                    }
                    let prev_xit = self.xit_hz;
                    scroll_slider_f32(ui, &mut self.xit_hz, -800.0..=800.0, "XIT");
                    if self.xit_hz != prev_xit {
                        let delta = self.xit_hz - prev_xit;
                        self.center_khz += delta as f64 / 1_000_000.0;
                    }
                });

                section_card(ui, |ui| {
                    section_heading(ui, "CW demod");
                    section_hint(ui, "Scroll over any control to adjust");
                    ui.horizontal_wrapped(|ui| {
                        for (label, hz) in BFO_PRESETS {
                            if ui.selectable_label(self.bfo_hz.round() == hz, label).clicked() {
                                self.bfo_hz = hz;
                            }
                        }
                    });
                    scroll_slider_f32(ui, &mut self.bfo_hz, 300.0..=1_200.0, "BFO tone");
                    scroll_slider_log_f32(
                        ui,
                        &mut self.passband_hz,
                        CW_PASSBAND_MIN_HZ..=CW_PASSBAND_MAX_HZ,
                        "Audio BW",
                    );
                    section_hint(ui, "Ctrl+scroll on plot: filter BW · drag cyan edges");
                    if self.is_kiwi {
                        ui.checkbox(&mut self.agc_on, "Kiwi RF AGC");
                    }
                    ui.checkbox(&mut self.software_agc, "Software AGC");
                });

                section_card(ui, |ui| {
                    section_heading(ui, "Anti-QRM");
                    ui.checkbox(&mut self.notch_enabled, "Notch (purple on plot)");
                    scroll_slider_f32(
                        ui,
                        &mut self.notch_offset_hz,
                        -5_000.0..=5_000.0,
                        "Notch offset",
                    );
                    scroll_slider_f32(
                        ui,
                        &mut self.notch_width_hz,
                        10.0..=200.0,
                        "Notch width",
                    );
                    if let Some(hover) = self.hover_offset_hz {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format!("Cursor {:.0} Hz", hover))
                                    .small()
                                    .color(MUTED),
                            );
                            if ui.small_button("Notch here").clicked() {
                                self.notch_offset_hz = hover as f32;
                                self.notch_enabled = true;
                            }
                        });
                    }
                    scroll_slider_f32(ui, &mut self.squelch, 0.0..=0.08, "Squelch");
                });

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
                        ui.checkbox(&mut self.audio_enabled, "Play on speakers");
                        scroll_slider_f32(ui, &mut self.volume, 0.0..=4.0, "Volume");
                        if let Some(audio) = &self.audio {
                            stat_row(ui, "Active", audio.device_name());
                            stat_row(ui, "Rate", format!("{} Hz", audio.output_rate()));
                        }
                    } else {
                        ui.colored_label(WARN, "Could not open output device");
                    }
                });

                collapsible_section(ui, "display", "Display", false, |ui| {
                    scroll_slider_f32(ui, &mut self.plot_view.zoom, 0.04..=1.0, "Zoom");
                    if ui.small_button("Full span").clicked() {
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
                    "Double-click: tune · Ctrl+scroll: filter BW · Shift+drag: pan",
                );
            });

        if self.is_kiwi && self.agc_on != self.last_agc_on {
            let _ = self.source.set_agc(self.agc_on);
            self.last_agc_on = self.agc_on;
        }

        self.apply_audio_device();
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
        let listen_center_hz = self.listen_offset_hz() as f64;

        let (_, spec_actions) = self.spectrum_widget.show(
            ui,
            &mut self.plot_interaction,
            &mut self.plot_view,
            self.sample_rate,
            200.0,
            &trace,
            self.passband_hz,
            true,
            listen_center_hz,
            self.ref_db,
            self.range_db,
            tune_preview_offset_hz,
            self.notch_enabled,
            self.notch_offset_hz,
            self.notch_width_hz,
            &mut self.hover_offset_hz,
        );
        plot_actions.extend(spec_actions);

        ui.add_space(4.0);

        if let Some(tex) = &self.texture {
            let wf_actions = self.waterfall_widget.show(
                ui,
                &mut self.plot_interaction,
                &mut self.plot_view,
                self.sample_rate,
                tex,
                self.passband_hz,
                true,
                listen_center_hz,
                tune_preview_offset_hz,
                self.notch_enabled,
                self.notch_offset_hz,
                self.notch_width_hz,
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

impl eframe::App for WaterfallApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if !self.themed {
            apply(&ctx);
            self.themed = true;
        }

        self.ingest();
        self.update_texture(&ctx);

        egui::Panel::right("controls")
            .resizable(true)
            .default_size(340.0)
            .show_inside(ui, |ui| self.controls_panel(ui));

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show_inside(ui, |ui| {
                self.central_panel(ui);
            });

        self.apply_radio_settings();

        ctx.request_repaint();
    }
}
