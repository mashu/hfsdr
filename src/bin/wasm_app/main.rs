//! hfsdr in the browser: the real DSP and the real waterfall shader, wasm32.
//!
//! This is deliberately not the desktop app recompiled. The desktop engine runs
//! a blocking loop on its own thread and reaches KiwiSDR over a TcpStream —
//! neither of which a browser tab can do. Until that engine becomes a step
//! function (the same restructuring a CLI frontend needs), this drives the
//! pipeline directly from the frame callback instead.
//!
//! What is real here: `SpectrumAnalyzer`, the windowing and coherent dBFS
//! normalisation, the view mapping, and `render::waterfall_gpu` — the same
//! shader the desktop build paints with, peak-hold and all. What is synthetic
//! is only the IQ, generated per frame so there is a signal to look at.

use eframe::egui;
use hfsdr::render::waterfall_gpu::{self, WaterfallCallback, RING_ROWS};
use hfsdr::{Complex32, SpectrumAnalyzer};

/// FFT size — 8 Hz bins at the synthetic 24 kHz rate.
const FFT_SIZE: usize = 4096;
const IQ_RATE: f32 = 24_000.0;

/// A CW carrier in the synthetic band.
struct Carrier {
    offset_hz: f32,
    amplitude: f32,
    /// Keying period in seconds; 0 means a steady carrier.
    dit_secs: f32,
}

const CARRIERS: &[Carrier] = &[
    Carrier { offset_hz: -7_200.0, amplitude: 0.30, dit_secs: 0.10 },
    Carrier { offset_hz: -3_100.0, amplitude: 0.18, dit_secs: 0.16 },
    Carrier { offset_hz: -800.0, amplitude: 0.45, dit_secs: 0.08 },
    Carrier { offset_hz: 1_500.0, amplitude: 0.12, dit_secs: 0.22 },
    Carrier { offset_hz: 4_600.0, amplitude: 0.25, dit_secs: 0.13 },
    Carrier { offset_hz: 9_000.0, amplitude: 0.08, dit_secs: 0.0 },
];

struct App {
    analyzer: SpectrumAnalyzer,
    iq: Vec<Complex32>,
    /// Sample index, so carrier phase and keying are continuous across frames.
    t: u64,
    rng: u32,
    gpu: bool,
    row_head: usize,
    pending: Vec<(usize, Vec<f32>)>,
    latest: Vec<f32>,
    ref_db: f32,
    range_db: f32,
    /// Visible span as a fraction of the full band, and its centre.
    zoom: f32,
    pan: f32,
    paused: bool,
    /// eframe resets visuals on the first frame, so apply the theme from `ui`.
    themed: bool,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let gpu = waterfall_gpu::install(cc);
        let mut app = Self {
            analyzer: SpectrumAnalyzer::new(FFT_SIZE, FFT_SIZE / 2),
            iq: Vec::with_capacity(4096),
            t: 0,
            rng: 0x2545_f491,
            gpu,
            row_head: 0,
            pending: Vec::new(),
            latest: vec![-120.0; FFT_SIZE],
            ref_db: -20.0,
            range_db: 80.0,
            zoom: 1.0,
            pan: 0.0,
            paused: false,
            themed: false,
        };
        // Pre-fill the ring so the page opens on a live-looking waterfall
        // instead of filling in over the first half-minute.
        app.prefill();
        app
    }

    /// Run enough of the pipeline to fill the whole ring before the first frame.
    fn prefill(&mut self) {
        let hop = FFT_SIZE / 2;
        for _ in 0..RING_ROWS {
            self.generate(hop);
            let iq = std::mem::take(&mut self.iq);
            let (latest, pending, head) = (&mut self.latest, &mut self.pending, &mut self.row_head);
            self.analyzer.process_limited(&iq, 1, |row| {
                latest.copy_from_slice(row);
                pending.push((*head, row.to_vec()));
                *head = (*head + 1) % RING_ROWS;
            });
            self.iq = iq;
        }
    }

    /// xorshift — deterministic, and no `rand` dependency in the wasm build.
    fn noise(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        (self.rng >> 8) as f32 / 8_388_608.0 - 1.0
    }

    /// Generate one frame of IQ: keyed CW carriers over a noise floor.
    fn generate(&mut self, samples: usize) {
        use std::f32::consts::TAU;
        self.iq.clear();
        self.iq.reserve(samples);
        for k in 0..samples {
            let n = self.t + k as u64;
            let secs = n as f32 / IQ_RATE;
            let (mut re, mut im) = (self.noise() * 0.010, self.noise() * 0.010);
            for c in CARRIERS {
                // Square keying with a soft edge, so the waterfall shows dits
                // and dahs rather than a solid line.
                let key = if c.dit_secs <= 0.0 {
                    1.0
                } else {
                    let phase = (secs / c.dit_secs).fract();
                    let on = phase < 0.55;
                    let edge = (phase.min(1.0 - phase) / 0.06).clamp(0.0, 1.0);
                    if on { edge } else { 0.0 }
                };
                if key <= 0.0 {
                    continue;
                }
                let w = TAU * c.offset_hz * secs;
                let a = c.amplitude * key;
                re += a * w.cos();
                im += a * w.sin();
            }
            self.iq.push(Complex32::new(re, im));
        }
        self.t += samples as u64;
    }

    fn pump(&mut self, dt: f32) {
        // Bound the batch so a backgrounded tab does not spike on return.
        let samples = ((dt * IQ_RATE) as usize).clamp(256, 8192);
        self.generate(samples);

        let iq = std::mem::take(&mut self.iq);
        let (latest, pending, head) = (&mut self.latest, &mut self.pending, &mut self.row_head);
        // Cap rows per frame: the analyzer retains the rest for the next call.
        self.analyzer.process_limited(&iq, 4, |row| {
            latest.copy_from_slice(row);
            pending.push((*head, row.to_vec()));
            *head = (*head + 1) % RING_ROWS;
        });
        self.iq = iq;
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if !self.themed {
            ctx.set_visuals(egui::Visuals::dark());
            self.themed = true;
        }
        let dt = ctx.input(|i| i.stable_dt).clamp(0.001, 0.1);
        if !self.paused {
            self.pump(dt);
        }

        egui::TopBottomPanel::top("bar").show_inside(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.strong("hfsdr");
                ui.separator();
                ui.label(if self.gpu { "GPU waterfall" } else { "no wgpu — CPU fallback unavailable here" });
                ui.separator();
                ui.add(egui::Slider::new(&mut self.ref_db, -80.0..=20.0).text("ref dB"));
                ui.add(egui::Slider::new(&mut self.range_db, 20.0..=120.0).text("range dB"));
                ui.add(egui::Slider::new(&mut self.zoom, 0.05..=1.0).text("zoom"));
                ui.add(egui::Slider::new(&mut self.pan, -0.5..=0.5).text("pan"));
                ui.toggle_value(&mut self.paused, "pause");
            });
            ui.label(
                egui::RichText::new(
                    "Synthetic 24 kHz IQ through the real spectrum analyzer and the same \
                     peak-hold waterfall shader the desktop build uses. Zoom and pan are \
                     uniform updates — they cost nothing.",
                )
                .small()
                .weak(),
            );
        });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            let rect = ui.available_rect_before_wrap();
            if self.gpu && rect.width() > 1.0 {
                // Visible window in storage-texture coordinates.
                let half = self.zoom.clamp(0.01, 1.0) as f64 / 2.0;
                let centre = 0.5 + self.pan.clamp(-0.5, 0.5) as f64;
                let uniforms = waterfall_gpu::uniforms_for(
                    centre - half,
                    centre + half,
                    self.row_head,
                    FFT_SIZE,
                    rect.width(),
                    self.ref_db,
                    self.range_db,
                );
                waterfall_gpu::paint(
                    ui.painter(),
                    rect,
                    WaterfallCallback {
                        uniforms,
                        new_rows: std::mem::take(&mut self.pending),
                        row_width: FFT_SIZE,
                    },
                );
            } else {
                ui.centered_and_justified(|ui| {
                    ui.label("This build needs WebGPU or WebGL via wgpu.");
                });
            }
        });

        ctx.request_repaint();
    }
}

fn main() {
    // Panics otherwise vanish into the console with no stack.
    std::panic::set_hook(Box::new(|info| {
        web_sys_log(&format!("hfsdr panic: {info}"));
    }));

    let opts = eframe::WebOptions::default();
    wasm_bindgen_futures::spawn_local(async {
        let document = web_sys::window()
            .expect("no window")
            .document()
            .expect("no document");
        let canvas = document
            .get_element_by_id("hfsdr_canvas")
            .expect("missing #hfsdr_canvas")
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("#hfsdr_canvas is not a canvas");

        let result = eframe::WebRunner::new()
            .start(canvas, opts, Box::new(|cc| Ok(Box::new(App::new(cc)))))
            .await;

        // Replace the loading text either way, so a failure is visible.
        if let Some(el) = document.get_element_by_id("loading") {
            match result {
                Ok(_) => el.remove(),
                Err(e) => el.set_text_content(Some(&format!("Failed to start: {e:?}"))),
            }
        }
    });
}

use wasm_bindgen::JsCast as _;

fn web_sys_log(msg: &str) {
    web_sys::console::error_1(&msg.into());
}
