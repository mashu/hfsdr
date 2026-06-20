//! Waterfall application state and rendering.

use std::collections::VecDeque;

use eframe::egui;
use egui::{Color32, Pos2, Sense, Stroke};
use hfsdr::{extract_passband_view, iq_to_audio, Complex32, Consumer, IqSource, SpectrumAnalyzer};

use crate::audio::AudioOutput;
use crate::colormap::db_to_colour;

pub const FFT_SIZE: usize = 1024;
pub const WATERFALL_ROWS: usize = 360;
pub const MAX_DRAIN_PER_FRAME: usize = 300_000;

const CW_PRESETS: [(&str, f64); 6] = [
    ("40m CW", 7_030_000.0),
    ("30m CW", 10_125_000.0),
    ("20m CW", 14_070_000.0),
    ("17m CW", 18_095_000.0),
    ("15m CW", 21_070_000.0),
    ("12m CW", 24_920_000.0),
];

const PASSBAND_PRESETS: [(&str, i32); 6] = [
    ("250 Hz", 250),
    ("500 Hz", 500),
    ("1 kHz", 1_000),
    ("2.5 kHz", 2_500),
    ("5 kHz", 5_000),
    ("10 kHz", 10_000),
];

pub struct WaterfallApp {
    source: Box<dyn IqSource>,
    iq: Consumer<Complex32>,
    analyzer: SpectrumAnalyzer,
    sample_rate: f32,
    center_khz: f64,
    last_center_khz: f64,
    is_kiwi: bool,
    passband_hz: i32,
    last_passband_hz: i32,
    agc_on: bool,
    last_agc_on: bool,

    rows: VecDeque<Vec<f32>>,
    latest: Vec<f32>,
    texture: Option<egui::TextureHandle>,

    ref_db: f32,
    range_db: f32,
    drain_scratch: Vec<Complex32>,
    iq_per_frame: usize,
    waterfall_rows: usize,

    audio: Option<AudioOutput>,
    audio_enabled: bool,
    volume: f32,
}

impl WaterfallApp {
    pub fn new(
        source: Box<dyn IqSource>,
        iq: Consumer<Complex32>,
        sample_rate: f32,
        center_hz: f64,
        is_kiwi: bool,
    ) -> Self {
        let passband_hz = if is_kiwi { 500 } else { 10_000 };
        let source_rate = sample_rate as u32;
        let audio = AudioOutput::try_open(source_rate);
        Self {
            source,
            iq,
            analyzer: SpectrumAnalyzer::new(FFT_SIZE, FFT_SIZE / 2),
            sample_rate,
            center_khz: center_hz / 1000.0,
            last_center_khz: center_hz / 1000.0,
            is_kiwi,
            passband_hz,
            last_passband_hz: passband_hz,
            agc_on: true,
            last_agc_on: true,
            rows: VecDeque::with_capacity(WATERFALL_ROWS),
            latest: vec![-120.0; FFT_SIZE],
            texture: None,
            ref_db: -20.0,
            range_db: 80.0,
            drain_scratch: Vec::with_capacity(MAX_DRAIN_PER_FRAME),
            iq_per_frame: 0,
            waterfall_rows: 0,
            audio,
            audio_enabled: true,
            volume: 2.0,
        }
    }

    fn display_span_hz(&self) -> f32 {
        if self.is_kiwi {
            self.passband_hz as f32
        } else {
            self.sample_rate
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
            if let Some(audio) = &mut self.audio {
                let mono = iq_to_audio(&self.drain_scratch);
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
    }

    fn apply_radio_settings(&mut self) {
        if (self.center_khz - self.last_center_khz).abs() > f64::EPSILON {
            let _ = self.source.tune(self.center_khz * 1000.0);
            self.last_center_khz = self.center_khz;
        }
        if self.is_kiwi && self.passband_hz != self.last_passband_hz {
            let half = self.passband_hz / 2;
            let _ = self.source.set_passband(-half, half);
            self.last_passband_hz = self.passband_hz;
        }
    }

    fn update_texture(&mut self, ctx: &egui::Context) {
        let span = self.display_span_hz();
        let view = extract_passband_view(&self.latest, self.sample_rate, span);
        let w = view.len().max(1);
        let h = WATERFALL_ROWS;
        let mut pixels = vec![Color32::BLACK; w * h];
        for (y, row) in self.rows.iter().enumerate() {
            let row_view = extract_passband_view(row, self.sample_rate, span);
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

    fn draw_spectrum(&self, ui: &mut egui::Ui, height: f32) {
        let span = self.display_span_hz();
        let view = extract_passband_view(&self.latest, self.sample_rate, span);

        let (resp, painter) =
            ui.allocate_painter(egui::vec2(ui.available_width(), height), Sense::hover());
        let rect = resp.rect;
        painter.rect_filled(rect, 0.0, Color32::from_rgb(8, 10, 16));

        let floor = self.ref_db - self.range_db;
        let n = view.len().max(1);
        let pts: Vec<Pos2> = view
            .iter()
            .enumerate()
            .map(|(i, &db)| {
                let x = if n <= 1 {
                    rect.center().x
                } else {
                    rect.left() + rect.width() * i as f32 / (n as f32 - 1.0)
                };
                let t = ((db - floor) / self.range_db).clamp(0.0, 1.0);
                let y = rect.bottom() - rect.height() * t;
                Pos2::new(x, y)
            })
            .collect();
        if pts.len() >= 2 {
            painter.add(egui::Shape::line(pts, Stroke::new(1.0, Color32::from_rgb(120, 220, 160))));
        }

        let cx = rect.center().x;
        painter.line_segment(
            [Pos2::new(cx, rect.top()), Pos2::new(cx, rect.bottom())],
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 80, 80, 120)),
        );

        let half_khz = span / 2000.0;
        painter.text(
            Pos2::new(rect.left(), rect.bottom()),
            egui::Align2::LEFT_BOTTOM,
            format!("-{half_khz:.2} kHz"),
            egui::FontId::proportional(11.0),
            Color32::GRAY,
        );
        painter.text(
            Pos2::new(rect.right(), rect.bottom()),
            egui::Align2::RIGHT_BOTTOM,
            format!("+{half_khz:.2} kHz"),
            egui::FontId::proportional(11.0),
            Color32::GRAY,
        );
    }

    fn controls_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("hfsdr");
        ui.label(format!("IQ rate: {:.1} kS/s", self.sample_rate / 1000.0));
        ui.label(format!(
            "view span: {:.1} kHz (passband)",
            self.display_span_hz() / 1000.0
        ));
        ui.separator();

        ui.label("CW band presets");
        ui.horizontal_wrapped(|ui| {
            for (label, hz) in CW_PRESETS {
                if ui.small_button(label).clicked() {
                    self.center_khz = hz / 1000.0;
                }
            }
        });
        ui.separator();

        ui.label("center (kHz)");
        ui.add(egui::DragValue::new(&mut self.center_khz).speed(0.1).suffix(" kHz"));
        ui.separator();

        if self.is_kiwi {
            ui.label("CW filter (IQ passband)");
            ui.horizontal_wrapped(|ui| {
                for (label, hz) in PASSBAND_PRESETS {
                    if ui.selectable_label(self.passband_hz == hz, label).clicked() {
                        self.passband_hz = hz;
                    }
                }
            });
            ui.add(
                egui::Slider::new(&mut self.passband_hz, 100..=10_000)
                    .logarithmic(true)
                    .suffix(" Hz"),
            );
            ui.checkbox(&mut self.agc_on, "AGC");
            ui.separator();
        }

        ui.label("audio (IQ → I channel)");
        if self.audio.is_some() {
            ui.checkbox(&mut self.audio_enabled, "play on speakers");
            ui.add(egui::Slider::new(&mut self.volume, 0.0..=8.0).text("volume"));
            if let Some(audio) = &self.audio {
                ui.label(format!(
                    "output: {} Hz",
                    audio.output_rate()
                ));
            }
        } else {
            ui.colored_label(Color32::YELLOW, "No audio device found");
        }
        ui.separator();

        ui.label("display");
        ui.add(egui::Slider::new(&mut self.ref_db, -120.0..=20.0).text("ref dB"));
        ui.add(egui::Slider::new(&mut self.range_db, 20.0..=140.0).text("range dB"));
        ui.separator();

        ui.label(format!("IQ / frame: {}", self.iq_per_frame));
        ui.label(format!("waterfall rows: {}", self.waterfall_rows));
        if let Some(rssi) = self.source.rssi_dbm() {
            ui.label(format!("S-meter: {rssi:.1} dBm"));
        }
        ui.label(format!("dropped: {}", self.source.dropped_samples()));
        if self.iq_per_frame == 0 {
            ui.colored_label(Color32::YELLOW, "Waiting for IQ data…");
        }

        if self.is_kiwi && self.agc_on != self.last_agc_on {
            let _ = self.source.set_agc(self.agc_on);
            self.last_agc_on = self.agc_on;
        }
        self.apply_radio_settings();
    }

    fn central_panel(&mut self, ui: &mut egui::Ui) {
        self.draw_spectrum(ui, 180.0);
        ui.add_space(2.0);
        if let Some(tex) = &self.texture {
            let size = egui::vec2(ui.available_width(), ui.available_height());
            ui.image(egui::load::SizedTexture::new(tex.id(), size));
        } else if self.waterfall_rows == 0 {
            ui.centered_and_justified(|ui| {
                ui.label("Waterfall will appear when IQ data arrives.");
            });
        }
    }
}

impl eframe::App for WaterfallApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.ingest();
        self.update_texture(&ctx);

        egui::Panel::right("controls")
            .default_width(280.0)
            .show(&ctx, |ui| self.controls_panel(ui));
        egui::CentralPanel::default().show(&ctx, |ui| self.central_panel(ui));

        ctx.request_repaint();
    }
}
