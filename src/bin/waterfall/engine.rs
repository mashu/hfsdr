//! Background DSP/audio engine.
//!
//! All real-time work lives here, off the UI thread: draining the IQ ring,
//! demodulating to audio, running the FFT, feeding the skimmer, and owning the
//! connection lifecycle (connect, stall/slow detection, exponential-backoff
//! auto-reconnect). The source and audio device are *created inside this thread*
//! so neither (a possibly `!Send` device handle or cpal stream) ever crosses a
//! thread boundary.
//!
//! The UI communicates by:
//! - writing [`EngineParams`] (DSP settings, volume) through a shared mutex,
//! - sending discrete [`EngineCommand`]s (connect, tune, ...),
//! - and reading [`EngineShared`] (spectrum rows, status, stats, spots).

use std::sync::Arc;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use hfsdr::{Complex32, CwChannelSettings, DecimFilterKind, FirDecimator, IngressWorker, IqAudioDemod, IqPlayback, IqRecorder, SpectrumAnalyzer, SpectrumFrontEnd, Spot, spectrum_hop, spectrum_plan};

use rayon::join;

use crate::af_scope::SCOPE_LEN;
use crate::audio::AudioOutput;
use crate::log;
use crate::skimmer::{ScpStatus, SkimmerHandle};
use crate::source::{connect, Connection, ConnectRequest, SourceKind};
use hfsdr::SkimmerConfig;

pub const FFT_SIZE: usize = 2048;
pub const FFT_HOP: usize = FFT_SIZE / 2;
pub const WATERFALL_ROWS: usize = 360;

/// Hard cap on samples drained per pump (wideband uses tail-only DSP below this).
const MAX_DRAIN_NARROW: usize = 1 << 16;
const MAX_DRAIN_WIDEBAND: usize = 1 << 16;
const MAX_SPECTRUM_ROWS_PER_PUMP: usize = 4;
const MIN_SPECTRUM_ROWS_WIDEBAND: usize = 2;
const MAX_SPECTRUM_ROWS_WIDEBAND: usize = 8;
/// Catch-up pumps when the IQ ring is backing up (Airspy at 384 kHz).
const MAX_CATCHUP_PUMPS: usize = 8;
const MAX_CATCHUP_PUMPS_LIGHT: usize = 2;
/// Wideband demod/FFT only need the freshest samples — not the full drain batch.
const WIDEBAND_IQ_THRESHOLD: f32 = 96_000.0;
const MAX_AUDIO_SAMPLES_WB: usize = 8192;
const MAX_FFT_INPUT_WB: usize = 20_480;
/// Slow decay on live peak-hold so stale carriers fade after tune-away.
const SKIMMER_PEAK_HOLD_DECAY_DB: f32 = 0.25;
/// Drop stale ring data when fill exceeds this (stay real-time under CPU pressure).
const RING_CATCHUP_FILL: f32 = 0.55;
const RING_CATCHUP_TARGET: f32 = 0.25;
/// No IQ for this long after the first sample triggers a reconnect.
const STALL_TIMEOUT_KIWI: Duration = Duration::from_secs(20);
const STALL_TIMEOUT_LOCAL: Duration = Duration::from_secs(12);
/// Wait this long for the first IQ sample before treating the link as stalled.
const KIWI_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(45);
/// Effective rate below this fraction of nominal for `SLOW_HOLD` flags "slow".
const SLOW_FRACTION: f32 = 0.7;
const SLOW_HOLD: Duration = Duration::from_secs(5);

/// Connection lifecycle, surfaced to the UI.
#[derive(Clone, Debug, PartialEq)]
pub enum ConnState {
    Disconnected,
    Connecting { label: String },
    Streaming,
    Reconnecting { attempt: u32, retry_in_s: f32 },
}

/// Live statistics for the status bar / diagnostics.
#[derive(Clone, Debug)]
pub struct EngineStats {
    pub sample_rate: f32,
    /// Native IQ passband width for panadapter axis (device rate or Kiwi passband).
    pub iq_passband_hz: f32,
    pub effective_sps: f32,
    pub last_drain: usize,
    pub dropped: u64,
    pub rssi_dbm: Option<f32>,
    pub snr_db: f32,
    pub audio_device: Option<String>,
    pub audio_rate: u32,
    pub slow: bool,
    pub is_kiwi: bool,
    pub skimmer_channels: usize,
    pub spectrum_rate: f32,
    pub spectrum_fft: usize,
    pub spectrum_decim: usize,
    pub spectrum_zoomed: bool,
    pub spectrum_rows_per_pump: usize,
    pub scp: ScpStatus,
    pub iq_recording: bool,
    pub iq_playback: bool,
    pub iq_capture_samples: u64,
    pub iq_capture_path: Option<String>,
    /// IQ ring fill 0..1 (smoothed); high = healthy headroom.
    pub iq_buffer_fill: f32,
    /// Seconds of IQ currently queued at nominal sample rate.
    pub iq_buffer_secs: f32,
    /// Smoothed peak |audio| for AF scope / RF gain tuning.
    pub audio_peak: f32,
    /// Smoothed RMS |audio|.
    pub audio_rms: f32,
    /// Current IQ-envelope AGC gain (1.0 when AGC off).
    pub agc_gain: f32,
    /// Smoothed IQ magnitude before AGC.
    pub agc_envelope: f32,
    /// KiwiSDR 2 reports `has_attn=1` when a hardware RF attenuator is present.
    pub kiwi_has_rf_attn: bool,
    /// Latest Kiwi hardware RF attenuator setting (dB).
    pub kiwi_rf_attn_db: f32,
}

impl Default for EngineStats {
    fn default() -> Self {
        Self {
            sample_rate: 12_000.0,
            iq_passband_hz: 12_000.0,
            effective_sps: 0.0,
            last_drain: 0,
            dropped: 0,
            rssi_dbm: None,
            snr_db: 0.0,
            audio_device: None,
            audio_rate: 0,
            slow: false,
            is_kiwi: false,
            skimmer_channels: 0,
            spectrum_rate: 12_000.0,
            spectrum_fft: FFT_SIZE,
            spectrum_decim: 1,
            spectrum_zoomed: false,
            spectrum_rows_per_pump: MIN_SPECTRUM_ROWS_WIDEBAND,
            scp: ScpStatus::default(),
            iq_recording: false,
            iq_playback: false,
            iq_capture_samples: 0,
            iq_capture_path: None,
            iq_buffer_fill: 0.0,
            iq_buffer_secs: 0.0,
            audio_peak: 0.0,
            audio_rms: 0.0,
            agc_gain: 1.0,
            agc_envelope: 0.0,
            kiwi_has_rf_attn: false,
            kiwi_rf_attn_db: 0.0,
        }
    }
}

/// UI-owned settings the engine reads each iteration.
#[derive(Clone, Debug)]
pub struct EngineParams {
    pub cw: CwChannelSettings,
    pub audio_enabled: bool,
    pub volume: f32,
    pub skimmer_enabled: bool,
    pub skimmer: SkimmerConfig,
    pub fft_size: usize,
    pub fft_auto: bool,
    /// Feed the full IQ drain batch to the spectrum analyzer (not just the recent tail).
    pub full_drain_spectrum: bool,
}

impl Default for EngineParams {
    fn default() -> Self {
        Self {
            cw: CwChannelSettings::default(),
            audio_enabled: true,
            volume: 1.0,
            skimmer_enabled: false,
            skimmer: SkimmerConfig::default(),
            fft_size: FFT_SIZE,
            fft_auto: true,
            full_drain_spectrum: false,
        }
    }
}

/// Data the engine publishes for the UI to render.
pub struct EngineShared {
    pub latest: Vec<f32>,
    pub new_rows: VecDeque<Vec<f32>>,
    pub state: ConnState,
    pub stats: EngineStats,
    pub spots: Vec<Spot>,
    pub last_error: Option<String>,
    pub rows_seq: u64,
    /// Recent demod audio samples for the AF scope (oldest first).
    pub audio_scope: Vec<f32>,
}

impl Default for EngineShared {
    fn default() -> Self {
        Self {
            latest: vec![-120.0; FFT_SIZE],
            new_rows: VecDeque::with_capacity(WATERFALL_ROWS),
            state: ConnState::Disconnected,
            stats: EngineStats::default(),
            spots: Vec::new(),
            last_error: None,
            rows_seq: 0,
            audio_scope: Vec::new(),
        }
    }
}

/// Discrete actions from the UI to the engine.
#[derive(Clone, Debug)]
pub enum EngineCommand {
    Connect(ConnectRequest),
    Disconnect,
    Tune(f64),
    SetRfAgc(bool),
    SetKiwiManGain(u8),
    SetKiwiRfAttn(f32),
    #[cfg(feature = "airspy")]
    SetAirspyAtt(u8),
    #[cfg(feature = "airspy")]
    SetAirspyLna(bool),
    #[cfg(feature = "airspy")]
    SetAirspyAgcThreshold(bool),
    #[cfg(feature = "airspy")]
    SetAirspyFrontendOptions(u32),
    #[cfg(feature = "airspy")]
    SetAirspyBiasTee(bool),
    #[cfg(feature = "rtlsdr")]
    SetRtlSdrRtlAgc(bool),
    #[cfg(feature = "rtlsdr")]
    SetRtlSdrManualGain(bool),
    #[cfg(feature = "rtlsdr")]
    SetRtlSdrTunerGain(i32),
    #[cfg(feature = "rtlsdr")]
    SetRtlSdrBiasTee(bool),
    #[cfg(feature = "rtlsdr")]
    SetRtlSdrPpm(i32),
    #[cfg(feature = "qmx")]
    SetQmxRfGain(u8),
    SetAudioDevice(Option<String>),
    ClearSkimmerSpots,
    ReloadScp,
    ReloadScpFrom(std::path::PathBuf),
    StartIqRecord(std::path::PathBuf),
    StopIqRecord,
    PlayIqFile(std::path::PathBuf),
    StopIqPlayback,
    Shutdown,
}

/// Snapshot from one engine poll (non-blocking).
#[derive(Clone, Debug)]
pub struct EnginePoll {
    pub state: ConnState,
    pub stats: EngineStats,
    pub spots: Vec<hfsdr::Spot>,
    pub rows: Vec<Vec<f32>>,
    pub latest: Vec<f32>,
    pub last_error: Option<String>,
    pub audio_scope: Vec<f32>,
}

/// UI-side handle to the engine thread.
pub struct EngineHandle {
    cmd_tx: Sender<EngineCommand>,
    shared: Arc<Mutex<EngineShared>>,
    params: Arc<Mutex<EngineParams>>,
    connect_cancel: Arc<AtomicBool>,
    join: Option<thread::JoinHandle<()>>,
}

impl EngineHandle {
    pub fn spawn() -> Self {
        let (cmd_tx, cmd_rx) = channel::<EngineCommand>();
        let shared = Arc::new(Mutex::new(EngineShared::default()));
        let params = Arc::new(Mutex::new(EngineParams::default()));
        let connect_cancel = Arc::new(AtomicBool::new(false));
        let shared_thread = Arc::clone(&shared);
        let params_thread = Arc::clone(&params);
        let connect_cancel_thread = Arc::clone(&connect_cancel);

        let join = thread::Builder::new()
            .name("engine".into())
            .spawn(move || {
                Engine::new(cmd_rx, shared_thread, params_thread, connect_cancel_thread).run();
            })
            .expect("spawn engine thread");

        Self {
            cmd_tx,
            shared,
            params,
            connect_cancel,
            join: Some(join),
        }
    }

    pub fn send(&self, cmd: EngineCommand) {
        let _ = self.cmd_tx.send(cmd);
    }

    /// Abort a blocking `connect()` from the UI thread (must run before or with Disconnect).
    pub fn abort_connect(&self) {
        self.connect_cancel.store(true, Ordering::Relaxed);
    }

    /// Overwrite the engine's view of UI settings (called once per UI frame).
    pub fn set_params(&self, params: EngineParams) {
        if let Ok(mut guard) = self.params.lock() {
            *guard = params;
        }
    }

    pub fn try_poll(&self) -> Option<EnginePoll> {
        let mut guard = self.shared.try_lock().ok()?;
        let rows: Vec<Vec<f32>> = guard.new_rows.drain(..).collect();
        Some(EnginePoll {
            state: guard.state.clone(),
            stats: guard.stats.clone(),
            spots: guard.spots.clone(),
            rows,
            latest: guard.latest.clone(),
            last_error: guard.last_error.clone(),
            audio_scope: guard.audio_scope.clone(),
        })
    }

    /// Signal shutdown and detach the worker thread — never blocks the UI thread.
    pub fn shutdown_now(&mut self) {
        self.abort_connect();
        self.send(EngineCommand::Shutdown);
        if let Some(h) = self.join.take() {
            // Dropping JoinHandle without join() detaches the thread.
            drop(h);
        }
    }
}

impl Drop for EngineHandle {
    fn drop(&mut self) {
        self.shutdown_now();
    }
}

struct AudioScopeRing {
    buf: Vec<f32>,
    write: usize,
    peak: f32,
    rms: f32,
}

impl AudioScopeRing {
    fn new() -> Self {
        Self {
            buf: vec![0.0; SCOPE_LEN],
            write: 0,
            peak: 0.0,
            rms: 0.0,
        }
    }

    fn push_block(&mut self, samples: &[f32]) {
        if samples.is_empty() {
            return;
        }
        let stride = (samples.len() / 48).max(1);
        let mut block_peak = 0.0f32;
        let mut block_sq = 0.0f32;
        let mut n = 0u32;
        for &s in samples.iter().step_by(stride) {
            self.buf[self.write] = s;
            self.write = (self.write + 1) % self.buf.len();
            let a = s.abs();
            block_peak = block_peak.max(a);
            block_sq += a * a;
            n += 1;
        }
        if n > 0 {
            let block_rms = (block_sq / n as f32).sqrt();
            self.peak = self.peak * 0.9 + block_peak * 0.1;
            self.rms = self.rms * 0.9 + block_rms * 0.1;
        }
    }

    fn ordered(&self) -> Vec<f32> {
        let len = self.buf.len();
        let mut out = Vec::with_capacity(len);
        for i in 0..len {
            out.push(self.buf[(self.write + i) % len]);
        }
        out
    }
}

/// Owned entirely by the engine thread.
struct Engine {
    cmd_rx: Receiver<EngineCommand>,
    shared: Arc<Mutex<EngineShared>>,
    params: Arc<Mutex<EngineParams>>,
    skimmer: SkimmerHandle,

    conn: Option<Connection>,
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
    first_iq_received: bool,
    reconnect_attempt: u32,
    retry_at: Option<Instant>,
    rate_window_start: Instant,
    rate_window_count: u64,
    cached_rate: f32,
    slow_since: Option<Instant>,
    running: bool,

    recorder: Option<IqRecorder>,
    recorder_samples: u64,
    playback: Option<IqPlayback>,
    iq_buffer_fill: f32,
    iq_buffer_secs: f32,
    iq_buffer_peak: f32,
    last_pump_got: usize,
    last_pump_at: Instant,
    last_spectrum_rows: usize,
    row_pool: Vec<Vec<f32>>,
    level_audio_peak: f32,
    level_audio_rms: f32,
    level_agc_gain: f32,
    level_agc_envelope: f32,
    level_audio_scope: Vec<f32>,

    /// Set by Disconnect/Cancel so an in-flight `connect()` can abort promptly.
    connect_cancel: Arc<AtomicBool>,
}

impl Engine {
    fn new(
        cmd_rx: Receiver<EngineCommand>,
        shared: Arc<Mutex<EngineShared>>,
        params: Arc<Mutex<EngineParams>>,
        connect_cancel: Arc<AtomicBool>,
    ) -> Self {
        Self {
            cmd_rx,
            shared,
            params,
            skimmer: SkimmerHandle::spawn("rx".into()),
            conn: None,
            request: None,
            audio: None,
            audio_device: None,
            demod: IqAudioDemod::new(),
            analyzer: SpectrumAnalyzer::new(FFT_SIZE, FFT_HOP),
            spectrum_front: SpectrumFrontEnd::new(12_000.0, 1, 0.0),
            spectrum_scratch: Vec::new(),
            drain: Vec::with_capacity(MAX_DRAIN_WIDEBAND),
            drain_decim: Vec::with_capacity(MAX_DRAIN_WIDEBAND),
            spectrum_ingress: FirDecimator::with_factor(384_000.0, 1, true, DecimFilterKind::LinearFir),
            spectrum_ingress_factor: 1,
            spectrum_ingress_rate: 384_000.0,
            spectrum_ingress_filter: DecimFilterKind::LinearFir,
            ingress_worker: Some(IngressWorker::spawn()),
            audio_scratch: Vec::new(),
            audio_scope: AudioScopeRing::new(),
            latest: vec![-120.0; FFT_SIZE],
            skimmer_peak_hold: vec![-120.0; FFT_SIZE],
            last_skimmer_center_hz: f64::NAN,
            fft_size: FFT_SIZE,
            spectrum_rate: 12_000.0,
            spectrum_decim: 1,
            spectrum_pan_hz: 0.0,
            spectrum_hop: FFT_SIZE / 2,
            pump_serial: 0,
            last_data: Instant::now(),
            connected_at: Instant::now(),
            first_iq_received: false,
            reconnect_attempt: 0,
            retry_at: None,
            rate_window_start: Instant::now(),
            rate_window_count: 0,
            cached_rate: 0.0,
            slow_since: None,
            running: true,
            recorder: None,
            recorder_samples: 0,
            playback: None,
            iq_buffer_fill: 0.0,
            iq_buffer_secs: 0.0,
            iq_buffer_peak: 0.0,
            last_pump_got: 0,
            last_pump_at: Instant::now(),
            last_spectrum_rows: MIN_SPECTRUM_ROWS_WIDEBAND,
            row_pool: Vec::new(),
            level_audio_peak: 0.0,
            level_audio_rms: 0.0,
            level_agc_gain: 1.0,
            level_agc_envelope: 0.0,
            level_audio_scope: Vec::new(),
            connect_cancel,
        }
    }

    fn run(&mut self) {
        while self.running {
            self.drain_commands();
            if !self.running {
                break;
            }

            let streaming = self.conn.is_some() || self.playback.is_some();
            if streaming {
                self.poll_handshake();
                let (ring_fill, _) = self.measure_iq_buffer();
                let iq_recording = self.recorder.is_some();
                let max_pumps = if iq_recording {
                    // Drain the ring aggressively — catch-up drops are disabled while recording.
                    if ring_fill > 0.2 {
                        MAX_CATCHUP_PUMPS * 4
                    } else {
                        MAX_CATCHUP_PUMPS
                    }
                } else if ring_fill > 0.35 {
                    if self
                        .params
                        .lock()
                        .map(|p| p.full_drain_spectrum)
                        .unwrap_or(false)
                    {
                        MAX_CATCHUP_PUMPS + 4
                    } else {
                        MAX_CATCHUP_PUMPS
                    }
                } else {
                    MAX_CATCHUP_PUMPS_LIGHT
                };
                let mut pumps = 0usize;
                loop {
                    let got = self.pump_stream();
                    pumps += 1;
                    self.drain_commands();
                    if !self.running || got == 0 || pumps >= max_pumps {
                        break;
                    }
                    let (fill, _) = self.measure_iq_buffer();
                    if fill < 0.2 {
                        break;
                    }
                }
                self.maybe_reconnect_on_stall();
                if self.last_pump_got == 0 {
                    thread::sleep(Duration::from_millis(3));
                }
            } else {
                self.maybe_retry_reconnect();
                let (sample_rate, _, _) = self.link_meta();
                let dt = self
                    .last_pump_at
                    .elapsed()
                    .as_secs_f32()
                    .clamp(0.001, 0.1);
                self.update_ring_utilization(sample_rate, (0.0, 0.0), 0, dt);
                self.last_pump_at = Instant::now();
                self.publish_stats(0);
                match self.cmd_rx.recv_timeout(Duration::from_millis(20)) {
                    Ok(cmd) => self.handle_command(cmd),
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => self.running = false,
                }
            }
        }
        // Clean shutdown: stop source so the reader thread exits.
        if let Some(conn) = &mut self.conn {
            let _ = conn.source.stop();
        }
    }

    fn drain_commands(&mut self) {
        loop {
            match self.cmd_rx.try_recv() {
                Ok(cmd) => self.handle_command(cmd),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.running = false;
                    break;
                }
            }
        }
    }

    fn handle_command(&mut self, cmd: EngineCommand) {
        match cmd {
            EngineCommand::Connect(req) => {
                self.request = Some(req.clone());
                self.reconnect_attempt = 0;
                self.retry_at = None;
                self.start_connect(&req);
            }
            EngineCommand::Disconnect => {
                self.connect_cancel.store(true, Ordering::Relaxed);
                self.teardown();
                self.request = None;
                self.retry_at = None;
                self.reconnect_attempt = 0;
                self.set_error(None);
                self.set_state(ConnState::Disconnected);
            }
            EngineCommand::Tune(hz) => {
                if let Some(conn) = &mut self.conn {
                    let _ = conn.source.tune(hz);
                    conn.center_hz = hz;
                }
                if let Some(req) = &mut self.request {
                    req.center_hz = hz;
                }
            }
            EngineCommand::SetRfAgc(on) => {
                if let Some(conn) = &mut self.conn {
                    let _ = conn.source.set_agc(on);
                }
            }
            EngineCommand::SetKiwiManGain(gain) => {
                if let Some(conn) = &mut self.conn {
                    let _ = conn.source.set_man_gain(gain);
                }
            }
            EngineCommand::SetKiwiRfAttn(db) => {
                if let Some(conn) = &mut self.conn {
                    let _ = conn.source.set_rf_attn_db(db);
                }
            }
            #[cfg(feature = "airspy")]
            EngineCommand::SetAirspyAtt(step) => {
                if let Some(conn) = &mut self.conn {
                    let _ = conn.source.set_hf_att(step);
                }
            }
            #[cfg(feature = "airspy")]
            EngineCommand::SetAirspyLna(on) => {
                if let Some(conn) = &mut self.conn {
                    let _ = conn.source.set_hf_lna(on);
                }
            }
            #[cfg(feature = "airspy")]
            EngineCommand::SetAirspyAgcThreshold(high) => {
                if let Some(conn) = &mut self.conn {
                    let _ = conn.source.set_hf_agc_threshold(high);
                }
            }
            #[cfg(feature = "airspy")]
            EngineCommand::SetAirspyFrontendOptions(flags) => {
                if let Some(conn) = &mut self.conn {
                    let _ = conn.source.set_frontend_options(flags);
                }
            }
            #[cfg(feature = "airspy")]
            EngineCommand::SetAirspyBiasTee(on) => {
                if let Some(conn) = &mut self.conn {
                    let _ = conn.source.set_bias_tee(on);
                }
            }
            #[cfg(feature = "rtlsdr")]
            EngineCommand::SetRtlSdrRtlAgc(on) => {
                if let Some(conn) = &mut self.conn {
                    let _ = conn.source.set_agc(on);
                }
            }
            #[cfg(feature = "rtlsdr")]
            EngineCommand::SetRtlSdrManualGain(manual) => {
                if let Some(conn) = &mut self.conn {
                    let _ = conn.source.set_tuner_gain_mode(manual);
                }
            }
            #[cfg(feature = "rtlsdr")]
            EngineCommand::SetRtlSdrTunerGain(gain_db10) => {
                if let Some(conn) = &mut self.conn {
                    let _ = conn.source.set_tuner_gain(gain_db10);
                }
            }
            #[cfg(feature = "rtlsdr")]
            EngineCommand::SetRtlSdrBiasTee(on) => {
                if let Some(conn) = &mut self.conn {
                    let _ = conn.source.set_bias_tee(on);
                }
            }
            #[cfg(feature = "rtlsdr")]
            EngineCommand::SetRtlSdrPpm(ppm) => {
                if let Some(conn) = &mut self.conn {
                    let _ = conn.source.set_freq_correction(ppm);
                }
            }
            #[cfg(feature = "qmx")]
            EngineCommand::SetQmxRfGain(db) => {
                if let Some(conn) = &mut self.conn {
                    let _ = conn.source.set_rf_gain_db(db);
                }
            }
            EngineCommand::SetAudioDevice(name) => {
                self.audio_device = name;
                self.reopen_audio();
            }
            EngineCommand::ClearSkimmerSpots => {
                self.skimmer.clear();
                self.reset_skimmer_peak_hold(self.fft_size);
            }
            EngineCommand::ReloadScp => {
                self.skimmer.reload_scp();
                self.publish_stats(0);
            }
            EngineCommand::ReloadScpFrom(path) => {
                self.skimmer.reload_scp_from(path);
                self.publish_stats(0);
            }
            EngineCommand::StartIqRecord(path) => {
                if self.recorder.is_some() {
                    return;
                }
                let (sr, center) = self
                    .conn
                    .as_ref()
                    .map(|c| (c.sample_rate as u32, c.center_hz))
                    .or_else(|| {
                        self.playback
                            .as_ref()
                            .map(|p| (p.meta().sample_rate, p.meta().center_hz))
                    })
                    .unwrap_or((12_000, 0.0));
                match IqRecorder::start(path.clone(), sr, center) {
                    Ok(rec) => {
                        self.recorder_samples = 0;
                        self.recorder = Some(rec);
                        log::info(format!("IQ recording → {}", path.display()));
                    }
                    Err(e) => self.set_error(Some(format!("IQ record failed: {e}"))),
                }
            }
            EngineCommand::StopIqRecord => {
                self.stop_recorder();
            }
            EngineCommand::PlayIqFile(path) => {
                self.teardown();
                self.request = None;
                match IqPlayback::open(path.clone()) {
                    Ok(pb) => {
                        let meta = pb.meta();
                        self.demod = IqAudioDemod::new();
                        self.audio_device_open(meta.sample_rate);
                        self.first_iq_received = true;
                        self.last_data = Instant::now();
                        self.rate_window_start = Instant::now();
                        self.rate_window_count = 0;
                        self.playback = Some(pb);
                        self.set_state(ConnState::Streaming);
                        self.set_error(None);
                        log::info(format!(
                            "IQ playback: {} ({:.1}s @ {} Hz)",
                            path.display(),
                            meta.duration_secs(),
                            meta.sample_rate
                        ));
                    }
                    Err(e) => {
                        self.set_error(Some(format!("IQ playback failed: {e}")));
                        self.set_state(ConnState::Disconnected);
                    }
                }
            }
            EngineCommand::StopIqPlayback => {
                self.playback = None;
                self.set_state(ConnState::Disconnected);
            }
            EngineCommand::Shutdown => {
                self.connect_cancel.store(true, Ordering::Relaxed);
                self.teardown();
                self.audio = None;
                self.ingress_worker.take();
                self.running = false;
            }
        }
    }

    fn start_connect(&mut self, req: &ConnectRequest) {
        self.connect_cancel.store(false, Ordering::Relaxed);
        self.teardown();
        if self.connect_cancel.load(Ordering::Relaxed) || self.request.is_none() {
            self.set_state(ConnState::Disconnected);
            return;
        }
        self.set_state(ConnState::Connecting { label: req.label() });
        self.drain_commands();
        if self.connect_cancel.load(Ordering::Relaxed) || self.request.is_none() {
            self.set_state(ConnState::Disconnected);
            return;
        }
        match connect(req, &self.connect_cancel) {
            Ok(conn) => {
                if self.connect_cancel.load(Ordering::Relaxed) || self.request.is_none() {
                    let mut conn = conn;
                    let _ = conn.source.stop();
                    self.set_state(ConnState::Disconnected);
                    return;
                }
                self.demod = IqAudioDemod::new();
                self.audio_device_open(conn.sample_rate as u32);
                self.last_data = Instant::now();
                self.connected_at = Instant::now();
                self.first_iq_received = !conn.is_kiwi;
                self.rate_window_start = Instant::now();
                self.rate_window_count = 0;
                self.reconnect_attempt = 0;
                self.retry_at = None;
                self.slow_since = None;
                self.conn = Some(conn);
                self.set_error(None);
                if self.conn.as_ref().is_some_and(|c| c.is_kiwi) {
                    self.set_state(ConnState::Connecting {
                        label: req.label(),
                    });
                } else {
                    self.set_state(ConnState::Streaming);
                }
                self.publish_stats(0);
            }
            Err(error) => {
                if self.connect_cancel.load(Ordering::Relaxed)
                    || self.request.is_none()
                    || error.contains("cancelled")
                {
                    self.set_state(ConnState::Disconnected);
                    return;
                }
                self.set_error(Some(error));
                self.schedule_reconnect();
                self.set_state(ConnState::Reconnecting {
                    attempt: self.reconnect_attempt,
                    retry_in_s: self.retry_secs(),
                });
            }
        }
    }

    fn teardown(&mut self) {
        if let Some(conn) = &mut self.conn {
            let _ = conn.source.stop();
        }
        self.conn = None;
        self.playback = None;
        self.iq_buffer_fill = 0.0;
        self.iq_buffer_secs = 0.0;
        self.iq_buffer_peak = 0.0;
        self.last_pump_got = 0;
        self.last_pump_at = Instant::now();
        self.reset_skimmer_peak_hold(self.fft_size);
        self.last_skimmer_center_hz = f64::NAN;
        self.stop_recorder();
    }

    fn reset_skimmer_peak_hold(&mut self, len: usize) {
        let len = len.max(1);
        if self.skimmer_peak_hold.len() != len {
            self.skimmer_peak_hold.resize(len, -120.0);
        } else {
            self.skimmer_peak_hold.fill(-120.0);
        }
    }

    fn touch_skimmer_center(&mut self, center_hz: f64) {
        if self.last_skimmer_center_hz.is_nan()
            || (center_hz - self.last_skimmer_center_hz).abs() > 1.0
        {
            self.reset_skimmer_peak_hold(self.fft_size);
            self.last_skimmer_center_hz = center_hz;
        }
    }

    fn stop_recorder(&mut self) {
        if let Some(rec) = self.recorder.take() {
            match rec.stop() {
                Ok(meta) => {
                    self.recorder_samples = meta.sample_count;
                    log::info(format!(
                        "IQ capture saved: {} samples ({:.1}s)",
                        meta.sample_count,
                        meta.duration_secs()
                    ));
                }
                Err(e) => log::error(format!("IQ capture finalize failed: {e}")),
            }
        }
    }

    fn audio_device_open(&mut self, _iq_rate: u32) {
        self.audio = match &self.audio_device {
            Some(name) => AudioOutput::try_open_named(name, 0)
                .or_else(|| AudioOutput::try_open_default(0)),
            None => AudioOutput::try_open_default(0),
        };
        if self.audio.is_none() {
            log::error("audio output unavailable (need PulseAudio/PipeWire/ALSA and F32 output)");
        }
    }

    fn reopen_audio(&mut self) {
        self.audio_device_open(0);
    }

    /// Drain and process available IQ; returns sample count processed.
    fn pump_stream(&mut self) -> usize {
        let params = self.params.lock().map(|g| g.clone()).unwrap_or_default();
        let dt = self
            .last_pump_at
            .elapsed()
            .as_secs_f32()
            .clamp(0.001, 0.1);
        let ring_before = self.measure_iq_buffer();

        self.drain.clear();
        let drain_cap = self.max_drain();
        if let Some(pb) = &mut self.playback {
            while self.drain.len() < drain_cap {
                match pb.pop() {
                    Some(s) => self.drain.push(s),
                    None => break,
                }
            }
            if pb.finished() && self.drain.is_empty() {
                self.playback = None;
                self.set_state(ConnState::Disconnected);
                log::info("IQ playback finished");
            }
        } else if let Some(conn) = &mut self.conn {
            // Never discard ring samples while recording — every sample must reach the file.
            if self.recorder.is_none() {
                let cap = conn.iq_ring_capacity.max(1);
                let slots = conn.iq.slots();
                let fill = slots as f32 / cap as f32;
                if fill >= RING_CATCHUP_FILL {
                    let target = (cap as f32 * RING_CATCHUP_TARGET) as usize;
                    while conn.iq.slots() > target {
                        let _ = conn.iq.pop();
                    }
                }
            }
            while self.drain.len() < drain_cap {
                match conn.iq.pop() {
                    Ok(s) => self.drain.push(s),
                    Err(_) => break,
                }
            }
        }
        let (device_rate, center_hz, _is_kiwi) = if let Some(pb) = &self.playback {
            let m = pb.meta();
            (m.sample_rate as f32, m.center_hz, false)
        } else {
            self.conn
                .as_ref()
                .map(|c| (c.device_sample_rate, c.center_hz, c.is_kiwi))
                .unwrap_or((12_000.0, 0.0, false))
        };
        let ingress_decim = self
            .conn
            .as_ref()
            .map(|c| c.iq_ingress_decim)
            .unwrap_or(1)
            .max(1);
        let got = self.drain.len();
        self.last_pump_got = got;
        self.update_ring_utilization(device_rate, ring_before, got, dt);
        self.last_pump_at = Instant::now();
        if got == 0 {
            self.publish_stats(0);
            return 0;
        }
        if let Some(rec) = &self.recorder {
            rec.push(&self.drain);
            self.recorder_samples += got as u64;
        }
        if !self.first_iq_received {
            self.first_iq_received = true;
            self.rate_window_start = Instant::now();
            self.rate_window_count = 0;
            self.set_state(ConnState::Streaming);
        }
        self.last_data = Instant::now();
        self.rate_window_count += got as u64;

        let spectrum_input_rate = if let Some(pb) = &self.playback {
            pb.meta().sample_rate as f32
        } else {
            self.conn
                .as_ref()
                .map(|c| c.sample_rate)
                .unwrap_or(device_rate)
        };

        self.sync_spectrum_chain(spectrum_input_rate, &params);
        self.touch_skimmer_center(center_hz);

        let cw = params.cw.clone();
        let wideband = device_rate > WIDEBAND_IQ_THRESHOLD;
        let batch = Arc::new(std::mem::take(&mut self.drain));
        self.drain = Vec::with_capacity(self.max_drain());

        if ingress_decim > 1 {
            let rebuild_ingress = ingress_decim != self.spectrum_ingress_factor
                || (device_rate - self.spectrum_ingress_rate).abs() > 1.0
                || cw.decim_filter != self.spectrum_ingress_filter;
            if rebuild_ingress {
                self.spectrum_ingress = FirDecimator::with_factor(
                    device_rate,
                    ingress_decim,
                    true,
                    cw.decim_filter,
                );
                self.spectrum_ingress_factor = ingress_decim;
                self.spectrum_ingress_rate = device_rate;
                self.spectrum_ingress_filter = cw.decim_filter;
            } else {
                self.spectrum_ingress.sync_filter(device_rate, cw.decim_filter);
            }
        }

        let use_ingress_worker = wideband
            && ingress_decim > 1
            && self.spectrum_decim <= 1
            && self.ingress_worker.as_ref().is_some_and(|w| {
                w.start(
                    Arc::clone(&batch),
                    device_rate,
                    ingress_decim,
                    cw.decim_filter,
                )
            });

        if use_ingress_worker {
            self.demod.process(
                self.demod_input(batch.as_slice(), device_rate),
                device_rate,
                &cw,
                &mut self.audio_scratch,
            );
            if let Some(decimated) = self.ingress_worker.as_ref().and_then(|w| w.finish()) {
                self.drain_decim = decimated;
            } else {
                self.spectrum_ingress
                    .decimate_block(batch.as_slice(), &mut self.drain_decim, false);
            }
        } else if wideband && ingress_decim > 1 && self.spectrum_decim <= 1 {
            let batch_demod = Arc::clone(&batch);
            let demod_input = self.demod_input(batch_demod.as_slice(), device_rate);
            let (demod, audio_scratch, ingress, decim_buf) = (
                &mut self.demod,
                &mut self.audio_scratch,
                &mut self.spectrum_ingress,
                &mut self.drain_decim,
            );
            join(
                || demod.process(demod_input, device_rate, &cw, audio_scratch),
                || ingress.decimate_block(batch.as_slice(), decim_buf, false),
            );
        } else {
            if ingress_decim > 1 {
                self.spectrum_ingress
                    .decimate_block(batch.as_slice(), &mut self.drain_decim, false);
            }
            if wideband && self.spectrum_decim > 1 {
                let ingress_base: &[Complex32] = if ingress_decim > 1 {
                    &self.drain_decim
                } else {
                    batch.as_slice()
                };
                let fft_base = self.spectrum_fft_slice(
                    ingress_base,
                    device_rate,
                    params.full_drain_spectrum,
                );
                let batch_demod = Arc::clone(&batch);
                let demod_input = self.demod_input(batch_demod.as_slice(), device_rate);
                let (demod, spectrum_front) = (&mut self.demod, &mut self.spectrum_front);
                let (audio_scratch, spectrum_scratch) =
                    (&mut self.audio_scratch, &mut self.spectrum_scratch);
                join(
                    || demod.process(demod_input, device_rate, &cw, audio_scratch),
                    || spectrum_front.process(fft_base, spectrum_scratch),
                );
            } else {
                self.demod.process(
                    self.demod_input(batch.as_slice(), device_rate),
                    device_rate,
                    &cw,
                    &mut self.audio_scratch,
                );
            }
        }

        let ingress_base: &[Complex32] = if ingress_decim > 1 {
            &self.drain_decim
        } else {
            batch.as_slice()
        };
        let fft_base = self.spectrum_fft_slice(
            ingress_base,
            device_rate,
            params.full_drain_spectrum,
        );
        if self.spectrum_decim > 1 {
            if !(wideband && self.spectrum_decim > 1) {
                self.spectrum_front
                    .process(fft_base, &mut self.spectrum_scratch);
            }
        } else {
            self.spectrum_scratch.clear();
            self.spectrum_scratch.extend_from_slice(fft_base);
        }

        if params.audio_enabled {
            if self.audio.is_none() {
                self.audio_device_open(0);
            }
            if let Some(audio) = &mut self.audio {
                let audio_rate = hfsdr::audio_sample_rate(device_rate, params.cw.decimation);
                audio.push(&self.audio_scratch, audio_rate as u32, params.volume);
            }
        }
        if !self.audio_scratch.is_empty() {
            self.audio_scope.push_block(&self.audio_scratch);
            self.level_audio_scope = self.audio_scope.ordered();
        }

        let agc_gain = if params.cw.agc.enabled {
            self.demod.agc_gain()
        } else {
            params.cw.agc.manual_gain
        };
        self.level_agc_gain = agc_gain;
        self.level_agc_envelope = self.demod.agc_envelope();
        self.level_audio_peak = self.audio_scope.peak;
        self.level_audio_rms = self.audio_scope.rms;

        let fft_input: &[Complex32] = &self.spectrum_scratch;
        let max_rows = self.adaptive_spectrum_rows(device_rate);
        self.last_spectrum_rows = max_rows;
        let playback = self.playback.is_some();
        let analyzer = &mut self.analyzer;
        let latest = &mut self.latest;
        let skimmer_peak_hold = &mut self.skimmer_peak_hold;
        let row_pool = &mut self.row_pool;
        let mut produced: Vec<Vec<f32>> = Vec::new();
        analyzer.process_limited(fft_input, max_rows, |row| {
            latest.copy_from_slice(row);
            if skimmer_peak_hold.len() != row.len() {
                skimmer_peak_hold.resize(row.len(), -120.0);
            }
            if playback {
                for (hold, &sample) in skimmer_peak_hold.iter_mut().zip(row.iter()) {
                    *hold = hold.max(sample);
                }
            } else {
                for (hold, &sample) in skimmer_peak_hold.iter_mut().zip(row.iter()) {
                    *hold = (*hold - SKIMMER_PEAK_HOLD_DECAY_DB).max(sample);
                }
            }
            let mut buf = row_pool
                .pop()
                .unwrap_or_else(|| vec![-120.0; row.len()]);
            if buf.len() != row.len() {
                buf.resize(row.len(), -120.0);
            }
            buf.copy_from_slice(row);
            produced.push(buf);
        });

        self.pump_serial = self.pump_serial.wrapping_add(1);
        let run_skimmer = params.skimmer_enabled;
        self.skimmer.set_enabled(run_skimmer);
        if run_skimmer {
            let spectrum_iq_rate = if let Some(pb) = &self.playback {
                pb.meta().sample_rate as f32
            } else if let Some(c) = &self.conn {
                c.sample_rate
            } else {
                device_rate
            };
            let (skimmer_iq, skimmer_iq_rate) = if ingress_decim > 1 && !self.drain_decim.is_empty() {
                (self.drain_decim.as_slice(), spectrum_iq_rate)
            } else {
                (batch.as_slice(), device_rate)
            };
            let is_kiwi = self.conn.as_ref().is_some_and(|c| c.is_kiwi);
            let throttle = if is_kiwi && skimmer_iq_rate <= 24_000.0 {
                2
            } else if skimmer_iq_rate > 96_000.0 {
                4
            } else if skimmer_iq_rate > 48_000.0 {
                2
            } else {
                1
            };
            if self.pump_serial % throttle == 0 {
                let mut cfg = params.skimmer.clone();
                cfg.source_label = "rx".to_string();
                self.skimmer.set_config(cfg);
                self.skimmer.submit(
                    skimmer_iq,
                    &self.skimmer_peak_hold,
                    skimmer_iq_rate,
                    self.spectrum_rate,
                    self.spectrum_pan_hz,
                    center_hz,
                );
            }
        }

        let snr = self.demod.snr_db();
        self.publish_rows(produced, snr, got);
        got
    }

    fn sync_spectrum_chain(&mut self, iq_rate: f32, params: &EngineParams) {
        // Always FFT the full passband; UI zoom/pan is a viewport crop on the waterfall.
        let _ = params;
        let (decim, fft, eff) = spectrum_plan(iq_rate, params.fft_size, params.fft_auto, iq_rate);
        self.spectrum_rate = eff;
        self.spectrum_decim = decim;
        self.spectrum_pan_hz = 0.0;
        self.spectrum_front.sync(iq_rate, decim, 0.0);
        let hop = spectrum_hop(fft, iq_rate);
        if fft != self.fft_size || hop != self.spectrum_hop {
            self.fft_size = fft;
            self.spectrum_hop = hop;
            self.analyzer = SpectrumAnalyzer::new(fft, hop);
            self.latest = vec![-120.0; fft];
            self.reset_skimmer_peak_hold(fft);
        }
    }

    fn spectrum_fft_slice<'a>(
        &self,
        samples: &'a [Complex32],
        rate: f32,
        full_drain: bool,
    ) -> &'a [Complex32] {
        if full_drain || rate <= WIDEBAND_IQ_THRESHOLD {
            samples
        } else {
            self.wideband_tail(samples, rate, self.max_fft_input())
        }
    }

    fn adaptive_spectrum_rows(&self, device_rate: f32) -> usize {
        if device_rate <= WIDEBAND_IQ_THRESHOLD {
            return MAX_SPECTRUM_ROWS_PER_PUMP;
        }
        let nominal = device_rate.max(1.0);
        let sps_ratio = (self.cached_rate / nominal).clamp(0.0, 1.25);
        let ring_headroom = 1.0 - self.iq_buffer_fill.clamp(0.0, 1.0);
        let score = (sps_ratio * 0.55 + ring_headroom * 0.45).clamp(0.0, 1.0);
        if score > 0.85 {
            MAX_SPECTRUM_ROWS_WIDEBAND
        } else if score > 0.65 {
            6
        } else if score > 0.45 {
            4
        } else {
            MIN_SPECTRUM_ROWS_WIDEBAND
        }
    }

    fn max_drain(&self) -> usize {
        let (sr, _, _) = self.link_meta();
        if sr > WIDEBAND_IQ_THRESHOLD {
            MAX_DRAIN_WIDEBAND
        } else if sr > 48_000.0 {
            MAX_DRAIN_NARROW
        } else {
            1 << 15
        }
    }

    fn max_fft_input(&self) -> usize {
        if self.link_meta().0 > WIDEBAND_IQ_THRESHOLD {
            (self.spectrum_hop * MAX_SPECTRUM_ROWS_WIDEBAND + self.fft_size).min(MAX_FFT_INPUT_WB)
        } else {
            usize::MAX
        }
    }

    fn wideband_tail<'a>(
        &self,
        samples: &'a [Complex32],
        rate: f32,
        max: usize,
    ) -> &'a [Complex32] {
        if rate <= WIDEBAND_IQ_THRESHOLD || samples.len() <= max {
            samples
        } else {
            &samples[samples.len() - max..]
        }
    }

    fn demod_input<'a>(&self, samples: &'a [Complex32], rate: f32) -> &'a [Complex32] {
        if rate > WIDEBAND_IQ_THRESHOLD {
            self.wideband_tail(samples, rate, MAX_AUDIO_SAMPLES_WB)
        } else {
            samples
        }
    }

    fn measure_iq_buffer(&self) -> (f32, f32) {
        if let Some(pb) = &self.playback {
            let fill = pb.buffer_fill();
            let secs = pb.buffer_secs();
            (fill, secs)
        } else if let Some(conn) = &self.conn {
            let cap = conn.iq_ring_capacity.max(1);
            let slots = conn.iq.slots();
            let fill = slots as f32 / cap as f32;
            let secs = slots as f32 / conn.device_sample_rate.max(1.0);
            (fill, secs)
        } else {
            (0.0, 0.0)
        }
    }

    fn update_ring_utilization(
        &mut self,
        sample_rate: f32,
        ring_before: (f32, f32),
        got: usize,
        dt: f32,
    ) {
        let (ring_fill, ring_secs) = ring_before;
        self.iq_buffer_peak = (self.iq_buffer_peak * 0.985).max(ring_fill);

        if self.playback.is_some() {
            // Disk playback: bar tracks ring occupancy (should stay high).
            let util = if got > 0 {
                ring_fill.max(0.75)
            } else {
                ring_fill * 0.5
            };
            self.iq_buffer_fill = self.iq_buffer_fill * 0.55 + util * 0.45;
            self.iq_buffer_secs = self.iq_buffer_secs * 0.55 + ring_secs * 0.45;
            return;
        }

        if self.conn.is_none() || !self.first_iq_received {
            self.iq_buffer_fill *= 0.8;
            self.iq_buffer_secs *= 0.8;
            return;
        }

        let nominal = sample_rate.max(1.0);
        let expected = nominal * dt;
        let throughput = if got == 0 {
            0.0
        } else {
            (got as f32 / expected).min(1.0)
        };

        // High when we consume a full pump batch; 0 when starved (got == 0).
        let util = if got == 0 {
            0.0
        } else {
            throughput
                .max(ring_fill)
                .max(self.iq_buffer_peak * 0.6)
        };

        if got == 0 {
            self.iq_buffer_fill *= 0.45;
        } else {
            self.iq_buffer_fill = self.iq_buffer_fill * 0.5 + util * 0.5;
        }
        let queued_secs = if got > 0 {
            ring_secs.max(got as f32 / nominal)
        } else {
            ring_secs
        };
        self.iq_buffer_secs = self.iq_buffer_secs * 0.5 + queued_secs * 0.5;
    }

    fn iq_buffer_stats(&self) -> (f32, f32) {
        (self.iq_buffer_fill, self.iq_buffer_secs)
    }

    fn publish_rows(&mut self, rows: Vec<Vec<f32>>, snr: f32, got: usize) {
        let spots = self.skimmer.spots();
        let scp = self.skimmer.scp_status();
        let channels = self.skimmer.active_channels();
        let dropped = self.conn.as_ref().map(|c| c.source.dropped_samples()).unwrap_or(0);
        let rssi = self.conn.as_ref().and_then(|c| c.source.rssi_dbm());
        let (sample_rate, _, is_kiwi) = self.link_meta();
        let (iq_recording, iq_playback, iq_capture_samples, iq_capture_path) = self.capture_ui();
        let effective = self.effective_rate(sample_rate);
        let slow = self.update_slow_flag(sample_rate, effective);
        let (audio_device, audio_rate) = self
            .audio
            .as_ref()
            .map(|a| (Some(a.device_name().to_string()), a.output_rate()))
            .unwrap_or((None, 0));
        let (iq_buffer_fill, iq_buffer_secs) = self.iq_buffer_stats();
        let (kiwi_has_rf_attn, kiwi_rf_attn_db) = self.kiwi_rf_stats();

        if let Ok(mut guard) = self.shared.lock() {
            if guard.latest.len() == self.latest.len() {
                guard.latest.copy_from_slice(&self.latest);
            } else {
                guard.latest = self.latest.clone();
            }
            for row in rows {
                if guard.new_rows.len() >= WATERFALL_ROWS {
                    guard.new_rows.pop_front();
                }
                guard.new_rows.push_back(row);
                guard.rows_seq = guard.rows_seq.wrapping_add(1);
            }
            guard.spots = spots;
            guard.stats = EngineStats {
                sample_rate: self
                    .conn
                    .as_ref()
                    .map(|c| c.sample_rate)
                    .unwrap_or(sample_rate),
                iq_passband_hz: self.iq_passband_hz(),
                effective_sps: effective,
                last_drain: got,
                dropped,
                rssi_dbm: rssi,
                snr_db: snr,
                audio_device,
                audio_rate,
                slow,
                is_kiwi,
                skimmer_channels: channels,
                spectrum_rate: self.spectrum_rate,
                spectrum_fft: self.fft_size,
                spectrum_decim: self.spectrum_decim,
                spectrum_zoomed: self.spectrum_decim > 1,
                spectrum_rows_per_pump: self.last_spectrum_rows,
                scp,
                iq_recording,
                iq_playback,
                iq_capture_samples,
                iq_capture_path,
                iq_buffer_fill,
                iq_buffer_secs,
                audio_peak: self.level_audio_peak,
                audio_rms: self.level_audio_rms,
                agc_gain: self.level_agc_gain,
                agc_envelope: self.level_agc_envelope,
                kiwi_has_rf_attn,
                kiwi_rf_attn_db,
            };
            guard.audio_scope = self.level_audio_scope.clone();
        }
    }

    fn link_meta(&self) -> (f32, f64, bool) {
        if let Some(pb) = &self.playback {
            let m = pb.meta();
            (m.sample_rate as f32, m.center_hz, false)
        } else if let Some(c) = &self.conn {
            (c.device_sample_rate, c.center_hz, c.is_kiwi)
        } else {
            (12_000.0, 0.0, false)
        }
    }

    fn iq_passband_hz(&self) -> f32 {
        if let Some(pb) = &self.playback {
            return pb.meta().sample_rate as f32;
        }
        if let Some(c) = &self.conn {
            if c.is_kiwi {
                hfsdr::kiwi_iq_half_hz(c.device_sample_rate as u32) as f32 * 2.0
            } else {
                c.device_sample_rate
            }
        } else {
            12_000.0
        }
    }

    fn capture_ui(&self) -> (bool, bool, u64, Option<String>) {
        (
            self.recorder.is_some(),
            self.playback.is_some(),
            self.recorder_samples,
            self.recorder
                .as_ref()
                .map(|r| r.path().display().to_string()),
        )
    }

    fn publish_stats(&mut self, got: usize) {
        let scp = self.skimmer.scp_status();
        let dropped = self.conn.as_ref().map(|c| c.source.dropped_samples()).unwrap_or(0);
        let rssi = self.conn.as_ref().and_then(|c| c.source.rssi_dbm());
        let (sample_rate, _, is_kiwi) = self.link_meta();
        let (iq_recording, iq_playback, iq_capture_samples, iq_capture_path) = self.capture_ui();
        let effective = self.effective_rate(sample_rate);
        let slow = self.update_slow_flag(sample_rate, effective);
        let (audio_device, audio_rate) = self
            .audio
            .as_ref()
            .map(|a| (Some(a.device_name().to_string()), a.output_rate()))
            .unwrap_or((None, 0));
        let (iq_buffer_fill, iq_buffer_secs) = self.iq_buffer_stats();
        let (kiwi_has_rf_attn, kiwi_rf_attn_db) = self.kiwi_rf_stats();
        if let Ok(mut guard) = self.shared.lock() {
            guard.stats = EngineStats {
                sample_rate: self
                    .conn
                    .as_ref()
                    .map(|c| c.sample_rate)
                    .unwrap_or(sample_rate),
                iq_passband_hz: self.iq_passband_hz(),
                effective_sps: effective,
                last_drain: got,
                dropped,
                rssi_dbm: rssi,
                snr_db: guard.stats.snr_db,
                audio_device,
                audio_rate,
                slow,
                is_kiwi,
                skimmer_channels: self.skimmer.active_channels(),
                spectrum_rate: self.spectrum_rate,
                spectrum_fft: self.fft_size,
                spectrum_decim: self.spectrum_decim,
                spectrum_zoomed: self.spectrum_decim > 1,
                spectrum_rows_per_pump: self.last_spectrum_rows,
                scp,
                iq_recording,
                iq_playback,
                iq_capture_samples,
                iq_capture_path,
                iq_buffer_fill,
                iq_buffer_secs,
                audio_peak: self.level_audio_peak,
                audio_rms: self.level_audio_rms,
                agc_gain: self.level_agc_gain,
                agc_envelope: self.level_agc_envelope,
                kiwi_has_rf_attn,
                kiwi_rf_attn_db,
            };
        }
    }

    fn kiwi_rf_stats(&self) -> (bool, f32) {
        self.conn
            .as_ref()
            .map(|c| (c.source.has_rf_attn(), c.source.rf_attn_db().unwrap_or(0.0)))
            .unwrap_or((false, 0.0))
    }

    fn effective_rate(&mut self, _nominal: f32) -> f32 {
        let elapsed = self.rate_window_start.elapsed().as_secs_f32();
        if elapsed >= 0.5 {
            let rate = self.rate_window_count as f32 / elapsed;
            self.rate_window_count = 0;
            self.rate_window_start = Instant::now();
            self.cached_rate = rate;
        }
        self.cached_rate
    }

    fn update_slow_flag(&mut self, nominal: f32, effective: f32) -> bool {
        if self.conn.is_none() || !self.first_iq_received {
            self.slow_since = None;
            return false;
        }
        if effective < SLOW_FRACTION * nominal {
            let since = *self.slow_since.get_or_insert_with(Instant::now);
            since.elapsed() >= SLOW_HOLD
        } else {
            self.slow_since = None;
            false
        }
    }

    fn poll_handshake(&mut self) {
        if self.first_iq_received {
            return;
        }
        let link_error = self
            .conn
            .as_ref()
            .and_then(|c| c.source.link_error());
        let alive = self
            .conn
            .as_ref()
            .is_some_and(|c| c.source.link_alive());
        if let Some(err) = link_error {
            self.fail_connection(err);
            return;
        }
        if !alive {
            self.fail_connection("Kiwi disconnected during handshake".into());
            return;
        }
        if self.connected_at.elapsed() > self.handshake_timeout() {
            self.fail_connection("Kiwi handshake timed out (no IQ data)".into());
        }
    }

    fn fail_connection(&mut self, reason: String) {
        self.teardown();
        if self.request.is_none() || self.connect_cancel.load(Ordering::Relaxed) {
            self.set_error(None);
            self.set_state(ConnState::Disconnected);
            return;
        }
        self.set_error(Some(reason));
        self.schedule_reconnect();
        self.set_state(ConnState::Reconnecting {
            attempt: self.reconnect_attempt,
            retry_in_s: self.retry_secs(),
        });
    }

    fn maybe_reconnect_on_stall(&mut self) {
        let link_error = self.conn.as_ref().and_then(|c| c.source.link_error());
        let reader_dead = self.conn.as_ref().is_some_and(|c| {
            c.is_kiwi && c.source.is_streaming() && !c.source.link_alive()
        });
        let stalled = if self.first_iq_received {
            self.last_data.elapsed() > self.stall_timeout()
        } else {
            self.connected_at.elapsed() > self.handshake_timeout()
        };
        if link_error.is_some() || reader_dead || stalled {
            let reason = link_error.unwrap_or_else(|| {
                if reader_dead {
                    "Kiwi reader stopped unexpectedly".to_string()
                } else if self.first_iq_received {
                    "connection stalled (no data)".to_string()
                } else {
                    "Kiwi handshake timed out (no IQ data)".to_string()
                }
            });
            self.fail_connection(reason);
        }
    }

    fn handshake_timeout(&self) -> Duration {
        if self.conn.as_ref().is_some_and(|c| c.is_kiwi) {
            KIWI_HANDSHAKE_TIMEOUT
        } else {
            STALL_TIMEOUT_LOCAL
        }
    }

    fn stall_timeout(&self) -> Duration {
        let is_kiwi = self.conn.as_ref().is_some_and(|c| c.is_kiwi);
        if is_kiwi {
            STALL_TIMEOUT_KIWI
        } else {
            STALL_TIMEOUT_LOCAL
        }
    }

    fn is_kiwi_request(&self) -> bool {
        self.request
            .as_ref()
            .is_some_and(|r| r.kind == SourceKind::Kiwi)
    }

    fn maybe_retry_reconnect(&mut self) {
        let Some(req) = self.request.clone() else {
            return;
        };
        let Some(at) = self.retry_at else {
            return;
        };
        let remaining = at.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            self.start_connect(&req);
        } else {
            self.set_state(ConnState::Reconnecting {
                attempt: self.reconnect_attempt,
                retry_in_s: remaining.as_secs_f32(),
            });
        }
    }

    fn schedule_reconnect(&mut self) {
        self.reconnect_attempt = self.reconnect_attempt.saturating_add(1);
        let busy = self
            .shared
            .lock()
            .ok()
            .and_then(|g| g.last_error.clone())
            .is_some_and(|e| e.to_ascii_lowercase().contains("busy"));
        let secs = if busy {
            15.0
        } else {
            self.retry_secs()
        };
        self.retry_at = Some(Instant::now() + Duration::from_secs_f32(secs));
    }

    fn retry_secs(&self) -> f32 {
        let base = if self.is_kiwi_request() { 3.0 } else { 2.0 };
        let exp = self.reconnect_attempt.saturating_sub(1).min(6);
        let max = if self.is_kiwi_request() { 60.0 } else { 30.0 };
        (base * 2u32.pow(exp) as f32).min(max)
    }

    fn set_state(&self, state: ConnState) {
        if let Ok(mut guard) = self.shared.lock() {
            guard.state = state;
        }
    }

    fn set_error(&self, error: Option<String>) {
        if let Ok(mut guard) = self.shared.lock() {
            guard.last_error = error;
        }
    }
}
