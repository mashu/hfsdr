//! Live waterfall + spectrum UI over any [`IqSource`], on egui's wgpu backend.
//!
//! Usage:
//!   waterfall airspy [sample_rate_hz] [center_hz]
//!   waterfall kiwi <host> [port] [center_hz]
//!
//! The waterfall is an egui texture updated each frame from a ring of FFT rows;
//! the spectrum trace is drawn with a custom painter. Both ride on wgpu.

use std::collections::VecDeque;

use eframe::egui;
use egui::{Color32, Pos2, Sense, Stroke};
use hfsdr::{AirspyHf, Complex32, Consumer, IqSource, KiwiSource, SpectrumAnalyzer};

const FFT_SIZE: usize = 1024;
const WATERFALL_ROWS: usize = 360;
const MAX_DRAIN_PER_FRAME: usize = 300_000;

fn main() -> eframe::Result {
    let (source, iq, sample_rate, center_hz) = match build_source() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("could not start source: {e}");
            std::process::exit(1);
        }
    };

    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default().with_inner_size([1100.0, 720.0]),
        ..Default::default()
    };
    eframe::run_native(
        "hfsdr waterfall",
        options,
        Box::new(move |_cc| {
            Ok(Box::new(WaterfallApp::new(source, iq, sample_rate, center_hz)))
        }),
    )
}

/// Build the requested source, tune it, and start streaming.
fn build_source() -> Result<(Box<dyn IqSource>, Consumer<Complex32>, f32, f64), String> {
    let args: Vec<String> = std::env::args().collect();
    let kind = args.get(1).map(String::as_str).unwrap_or("airspy");

    match kind {
        "kiwi" => {
            let host = args.get(2).cloned().ok_or("kiwi: missing host")?;
            let port = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(8073u16);
            let center = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(7_030_000.0);
            let mut src = KiwiSource::new(host, port);
            src.tune(center).map_err(|e| e.to_string())?;
            let sr = src.sample_rate() as f32;
            let iq = src.start().map_err(|e| e.to_string())?;
            Ok((Box::new(src), iq, sr, center))
        }
        _ => {
            let mut src = AirspyHf::open().map_err(|e| e.to_string())?;
            let sr = args
                .get(2)
                .and_then(|s| s.parse().ok())
                .or_else(|| src.sample_rates().first().copied())
                .unwrap_or(768_000);
            let center = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(7_030_000.0);
            src.set_sample_rate(sr).map_err(|e| e.to_string())?;
            src.set_lib_dsp(true).ok();
            src.tune(center).map_err(|e| e.to_string())?;
            let iq = src.start().map_err(|e| e.to_string())?;
            Ok((Box::new(src), iq, sr as f32, center))
        }
    }
}

struct WaterfallApp {
    source: Box<dyn IqSource>,
    iq: Consumer<Complex32>,
    analyzer: SpectrumAnalyzer,
    sample_rate: f32,
    center_khz: f64,
    last_center_khz: f64,

    rows: VecDeque<Vec<f32>>, // newest at front, dB values
    latest: Vec<f32>,
    texture: Option<egui::TextureHandle>,

    ref_db: f32,
    range_db: f32,
    drain_scratch: Vec<Complex32>,
}

impl WaterfallApp {
    fn new(
        source: Box<dyn IqSource>,
        iq: Consumer<Complex32>,
        sample_rate: f32,
        center_hz: f64,
    ) -> Self {
        Self {
            source,
            iq,
            analyzer: SpectrumAnalyzer::new(FFT_SIZE, FFT_SIZE / 2),
            sample_rate,
            center_khz: center_hz / 1000.0,
            last_center_khz: center_hz / 1000.0,
            rows: VecDeque::with_capacity(WATERFALL_ROWS),
            latest: vec![-120.0; FFT_SIZE],
            texture: None,
            ref_db: -20.0,
            range_db: 80.0,
            drain_scratch: Vec::with_capacity(MAX_DRAIN_PER_FRAME),
        }
    }

    /// Pull available IQ from the ring and turn it into new waterfall rows.
    fn ingest(&mut self) {
        self.drain_scratch.clear();
        while self.drain_scratch.len() < MAX_DRAIN_PER_FRAME {
            match self.iq.pop() {
                Ok(s) => self.drain_scratch.push(s),
                Err(_) => break,
            }
        }
        if self.drain_scratch.is_empty() {
            return;
        }
        let rows = &mut self.rows;
        let latest = &mut self.latest;
        self.analyzer.process(&self.drain_scratch, |row| {
            latest.copy_from_slice(row);
            rows.push_front(row.to_vec());
            if rows.len() > WATERFALL_ROWS {
                rows.pop_back();
            }
        });
    }

    /// Map a dB value to a colour using the current reference/range.
    fn colour(&self, db: f32) -> Color32 {
        let floor = self.ref_db - self.range_db;
        let t = ((db - floor) / self.range_db).clamp(0.0, 1.0);
        // Five-stop perceptual-ish ramp: black -> blue -> magenta -> orange -> white.
        const STOPS: [(f32, f32, f32); 5] = [
            (0.0, 0.0, 0.0),
            (0.15, 0.1, 0.6),
            (0.75, 0.1, 0.55),
            (1.0, 0.6, 0.05),
            (1.0, 1.0, 0.9),
        ];
        let scaled = t * (STOPS.len() as f32 - 1.0);
        let i = scaled.floor() as usize;
        let j = (i + 1).min(STOPS.len() - 1);
        let f = scaled - i as f32;
        let lerp = |a: f32, b: f32| ((a + (b - a) * f) * 255.0) as u8;
        Color32::from_rgb(
            lerp(STOPS[i].0, STOPS[j].0),
            lerp(STOPS[i].1, STOPS[j].1),
            lerp(STOPS[i].2, STOPS[j].2),
        )
    }

    /// Rebuild the waterfall texture from the current row ring.
    fn update_texture(&mut self, ctx: &egui::Context) {
        let w = FFT_SIZE;
        let h = WATERFALL_ROWS;
        let mut pixels = vec![Color32::BLACK; w * h];
        for (y, row) in self.rows.iter().enumerate() {
            let base = y * w;
            for x in 0..w {
                pixels[base + x] = self.colour(row[x]);
            }
        }
        let image = egui::ColorImage { size: [w, h], pixels };
        match &mut self.texture {
            Some(tex) => tex.set(image, egui::TextureOptions::LINEAR),
            none => {
                *none = Some(ctx.load_texture("waterfall", image, egui::TextureOptions::LINEAR));
            }
        }
    }

    fn draw_spectrum(&self, ui: &mut egui::Ui, height: f32) {
        let (resp, painter) =
            ui.allocate_painter(egui::vec2(ui.available_width(), height), Sense::hover());
        let rect = resp.rect;
        painter.rect_filled(rect, 0.0, Color32::from_rgb(8, 10, 16));

        let floor = self.ref_db - self.range_db;
        let n = self.latest.len();
        let pts: Vec<Pos2> = self
            .latest
            .iter()
            .enumerate()
            .map(|(i, &db)| {
                let x = rect.left() + rect.width() * i as f32 / (n as f32 - 1.0);
                let t = ((db - floor) / self.range_db).clamp(0.0, 1.0);
                let y = rect.bottom() - rect.height() * t;
                Pos2::new(x, y)
            })
            .collect();
        painter.add(egui::Shape::line(pts, Stroke::new(1.0, Color32::from_rgb(120, 220, 160))));

        // Centre line (DC / tuned frequency).
        let cx = rect.center().x;
        painter.line_segment(
            [Pos2::new(cx, rect.top()), Pos2::new(cx, rect.bottom())],
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 80, 80, 120)),
        );
    }
}

impl eframe::App for WaterfallApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.ingest();
        self.update_texture(ctx);

        egui::SidePanel::right("controls").show(ctx, |ui| {
            ui.heading("hfsdr");
            ui.label(format!("rate: {:.1} kS/s", self.sample_rate / 1000.0));
            let span_khz = self.sample_rate / 1000.0;
            ui.label(format!("span: {span_khz:.1} kHz"));
            ui.separator();

            ui.label("center (kHz)");
            ui.add(egui::DragValue::new(&mut self.center_khz).speed(0.1).suffix(" kHz"));
            if (self.center_khz - self.last_center_khz).abs() > f64::EPSILON {
                let _ = self.source.tune(self.center_khz * 1000.0);
                self.last_center_khz = self.center_khz;
            }
            ui.separator();

            ui.label("display");
            ui.add(egui::Slider::new(&mut self.ref_db, -120.0..=20.0).text("ref dB"));
            ui.add(egui::Slider::new(&mut self.range_db, 20.0..=140.0).text("range dB"));
            ui.separator();

            ui.label(format!("dropped: {}", self.source.dropped_samples()));
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            self.draw_spectrum(ui, 180.0);
            ui.add_space(2.0);
            if let Some(tex) = &self.texture {
                let size = egui::vec2(ui.available_width(), ui.available_height());
                ui.image(egui::load::SizedTexture::new(tex.id(), size));
            }
        });

        ctx.request_repaint();
    }
}
