//! Engine worker: state + split `impl` blocks (same parent module → private field access).

mod commands;
mod connection;
mod pump;
mod reconnect;
mod spectrum;
mod worker;

#[cfg(test)]
mod tests;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Receiver;
use std::sync::Mutex;
use std::time::Instant;

use hfsdr::{
    Complex32, DecimFilterKind, FirDecimator, IngressWorker, IqAudioDemod, IqPlayback, IqRecorder,
    PipelineMetrics, SpectrumAnalyzer, SpectrumFrontEnd,
};

use crate::audio::AudioOutput;
use crate::skimmer::SkimmerHandle;
use crate::source::{Connection, ConnectRequest};

use super::audio::AudioScopeRing;
use super::types::{EngineCommand, EngineParams, EngineShared};

/// Owned entirely by the engine thread.
pub(crate) struct Engine {
    cmd_rx: Receiver<EngineCommand>,
    shared: Arc<Mutex<EngineShared>>,
    params: Arc<Mutex<EngineParams>>,
    skimmer: SkimmerHandle,

    pub(crate) conn: Option<Connection>,
    request: Option<ConnectRequest>,
    audio: Option<AudioOutput>,
    audio_device: Option<String>,
    demod: IqAudioDemod,
    analyzer: SpectrumAnalyzer,
    spectrum_front: SpectrumFrontEnd,
    spectrum_scratch: Vec<Complex32>,

    drain: Vec<Complex32>,
    drain_decim: Vec<Complex32>,
    spectrum_ingress: FirDecimator,
    spectrum_ingress_factor: usize,
    spectrum_ingress_rate: f32,
    spectrum_ingress_filter: DecimFilterKind,
    ingress_worker: Option<IngressWorker>,
    audio_scratch: Vec<f32>,
    audio_scope: AudioScopeRing,
    latest: Vec<f32>,
    /// Max-hold spectrum for skimmer peak picking (CW carriers are intermittent).
    skimmer_peak_hold: Vec<f32>,
    last_skimmer_center_hz: f64,
    fft_size: usize,
    spectrum_rate: f32,
    spectrum_decim: usize,
    spectrum_pan_hz: f32,
    spectrum_hop: usize,
    pump_serial: u64,

    last_data: Instant,
    connected_at: Instant,
    pub(crate) first_iq_received: bool,
    reconnect_attempt: u32,
    retry_at: Option<Instant>,
    rate_window_start: Instant,
    rate_window_count: u64,
    cached_rate: f32,
    slow_since: Option<Instant>,
    running: bool,

    recorder: Option<IqRecorder>,
    recorder_samples: u64,
    pub(crate) playback: Option<IqPlayback>,
    iq_buffer_fill: f32,
    iq_buffer_secs: f32,
    iq_buffer_peak: f32,
    last_pump_got: usize,
    last_pump_at: Instant,
    last_iq_dropped: u64,
    last_spectrum_rows: usize,
    row_pool: Vec<Vec<f32>>,
    level_audio_peak: f32,
    level_audio_rms: f32,
    level_agc_gain: f32,
    level_agc_envelope: f32,
    level_iq_rf: f32,
    level_audio_scope: Vec<f32>,

    pipeline_avg: PipelineMetrics,
    last_perf_log: Instant,
    last_pipeline: PipelineMetrics,

    /// Set by Disconnect/Cancel so an in-flight `connect()` can abort promptly.
    connect_cancel: Arc<AtomicBool>,
}
