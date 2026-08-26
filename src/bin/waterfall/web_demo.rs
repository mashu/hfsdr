//! Synthetic IQ source for the browser build.
//!
//! The engine thread cannot run in a tab and none of the real sources are
//! reachable from one, so without this the UI would render over a dead
//! waterfall. Everything downstream is genuine: the IQ goes through the same
//! [`SpectrumAnalyzer`] the desktop build uses, and the result is handed to the
//! app through the same [`EnginePoll`] path a live engine fills.

use std::f32::consts::TAU;

use hfsdr::{Complex32, SpectrumAnalyzer};

use crate::engine::{ConnState, EnginePoll, EngineStats, FFT_SIZE};

/// Synthetic passband rate. Matches a KiwiSDR-ish narrow span so the band
/// scale, filter overlay and tuning controls all read sensibly.
const IQ_RATE: f32 = 12_000.0;

/// A CW signal in the synthetic band.
struct Carrier {
    offset_hz: f32,
    amplitude: f32,
    /// Dit length in seconds; 0 is an unkeyed carrier.
    dit_secs: f32,
}

const CARRIERS: &[Carrier] = &[
    Carrier { offset_hz: -4_100.0, amplitude: 0.22, dit_secs: 0.10 },
    Carrier { offset_hz: -2_300.0, amplitude: 0.12, dit_secs: 0.16 },
    Carrier { offset_hz: -650.0, amplitude: 0.40, dit_secs: 0.08 },
    Carrier { offset_hz: 900.0, amplitude: 0.16, dit_secs: 0.13 },
    Carrier { offset_hz: 2_800.0, amplitude: 0.09, dit_secs: 0.21 },
    Carrier { offset_hz: 4_500.0, amplitude: 0.06, dit_secs: 0.0 },
];

pub struct WebDemoSource {
    analyzer: SpectrumAnalyzer,
    iq: Vec<Complex32>,
    /// Sample index, so phase and keying stay continuous across frames.
    t: u64,
    rng: u32,
    latest: Vec<f32>,
}

impl Default for WebDemoSource {
    fn default() -> Self {
        Self {
            analyzer: SpectrumAnalyzer::new(FFT_SIZE, FFT_SIZE / 2),
            iq: Vec::new(),
            t: 0,
            rng: 0x2545_f491,
            latest: vec![-120.0; FFT_SIZE],
        }
    }
}

impl WebDemoSource {
    /// xorshift — deterministic, and avoids a `rand` dependency in the wasm build.
    fn noise(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        (self.rng >> 8) as f32 / 8_388_608.0 - 1.0
    }

    fn generate(&mut self, samples: usize) {
        self.iq.clear();
        self.iq.reserve(samples);
        for k in 0..samples {
            let n = self.t + k as u64;
            let secs = n as f32 / IQ_RATE;
            let (mut re, mut im) = (self.noise() * 0.012, self.noise() * 0.012);
            for c in CARRIERS {
                // Square keying with a soft edge, so dits and dahs are visible
                // rather than a solid line.
                let key = if c.dit_secs <= 0.0 {
                    1.0
                } else {
                    let phase = (secs / c.dit_secs).fract();
                    if phase < 0.55 {
                        (phase.min(1.0 - phase) / 0.06).clamp(0.0, 1.0)
                    } else {
                        0.0
                    }
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

    /// Fill a whole waterfall's worth of history in one poll.
    ///
    /// The scroll pacing applies a bounded number of rows per frame, so without
    /// this the page opens on an empty waterfall that takes half a minute to
    /// fill. The first texture build composes straight from the row history, so
    /// one poll carrying `rows` of it lands fully formed.
    pub fn prefill(&mut self, rows: usize, prev: &EngineStats) -> EnginePoll {
        let hop = FFT_SIZE / 2;
        let mut out: Vec<Vec<f32>> = Vec::with_capacity(rows);
        for _ in 0..rows {
            self.generate(hop);
            let iq = std::mem::take(&mut self.iq);
            let latest = &mut self.latest;
            self.analyzer.process_limited(&iq, 1, |row| {
                latest.copy_from_slice(row);
                out.push(row.to_vec());
            });
            self.iq = iq;
        }
        let mut poll = self.step(0.02, prev);
        // `step` appends its own rows; put the history in front of them.
        out.append(&mut poll.rows);
        poll.rows = out;
        poll
    }

    /// Produce one frame's worth of spectrum rows as an [`EnginePoll`].
    pub fn step(&mut self, dt: f32, prev: &EngineStats) -> EnginePoll {
        // Bound the batch so a backgrounded tab does not spike on return.
        let samples = ((dt * IQ_RATE) as usize).clamp(128, 4096);
        self.generate(samples);

        let iq = std::mem::take(&mut self.iq);
        let mut rows: Vec<Vec<f32>> = Vec::new();
        let latest = &mut self.latest;
        self.analyzer.process_limited(&iq, 4, |row| {
            latest.copy_from_slice(row);
            rows.push(row.to_vec());
        });
        self.iq = iq;

        let mut stats = prev.clone();
        stats.sample_rate = IQ_RATE;
        stats.iq_passband_hz = IQ_RATE;
        stats.spectrum_rate = IQ_RATE;
        stats.spectrum_fft = FFT_SIZE;
        stats.last_drain = self.iq.len();

        EnginePoll {
            state: ConnState::Streaming,
            stats,
            rows,
            latest: self.latest.clone(),
            last_error: None,
            audio_scope: Vec::new(),
            audio_waveform: Vec::new(),
        }
    }
}
