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

use std::collections::VecDeque;
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use hfsdr::{Complex32, CwChannelSettings, IqAudioDemod, SpectrumAnalyzer, Spot};

use crate::audio::AudioOutput;
use crate::skimmer::SkimmerHandle;
use crate::source::{connect, Connection, ConnectRequest};

pub const FFT_SIZE: usize = 2048;
pub const FFT_HOP: usize = FFT_SIZE / 2;
pub const WATERFALL_ROWS: usize = 360;

/// Hard cap on samples processed per engine iteration (bounds latency spikes).
const MAX_DRAIN: usize = 1 << 16;
/// No IQ for this long while streaming triggers a reconnect.
const STALL_TIMEOUT: Duration = Duration::from_secs(3);
/// Effective rate below this fraction of nominal for `SLOW_HOLD` flags "slow".
const SLOW_FRACTION: f32 = 0.7;
const SLOW_HOLD: Duration = Duration::from_secs(2);

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
}

impl Default for EngineStats {
    fn default() -> Self {
        Self {
            sample_rate: 12_000.0,
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
    pub fft_size: usize,
}

impl Default for EngineParams {
    fn default() -> Self {
        Self {
            cw: CwChannelSettings::default(),
            audio_enabled: true,
            volume: 1.0,
            skimmer_enabled: false,
            fft_size: FFT_SIZE,
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
    SetAudioDevice(Option<String>),
    Shutdown,
}

/// UI-side handle to the engine thread.
pub struct EngineHandle {
    cmd_tx: Sender<EngineCommand>,
    shared: Arc<Mutex<EngineShared>>,
    params: Arc<Mutex<EngineParams>>,
    join: Option<thread::JoinHandle<()>>,
}

impl EngineHandle {
    pub fn spawn() -> Self {
        let (cmd_tx, cmd_rx) = channel::<EngineCommand>();
        let shared = Arc::new(Mutex::new(EngineShared::default()));
        let params = Arc::new(Mutex::new(EngineParams::default()));
        let shared_thread = Arc::clone(&shared);
        let params_thread = Arc::clone(&params);

        let join = thread::Builder::new()
            .name("engine".into())
            .spawn(move || {
                Engine::new(cmd_rx, shared_thread, params_thread).run();
            })
            .expect("spawn engine thread");

        Self {
            cmd_tx,
            shared,
            params,
            join: Some(join),
        }
    }

    pub fn send(&self, cmd: EngineCommand) {
        let _ = self.cmd_tx.send(cmd);
    }

    /// Overwrite the engine's view of UI settings (called once per UI frame).
    pub fn set_params(&self, params: EngineParams) {
        if let Ok(mut guard) = self.params.lock() {
            *guard = params;
        }
    }

    pub fn with_shared<R>(&self, f: impl FnOnce(&mut EngineShared) -> R) -> R {
        let mut guard = self.shared.lock().expect("engine shared poisoned");
        f(&mut guard)
    }
}

impl Drop for EngineHandle {
    fn drop(&mut self) {
        self.send(EngineCommand::Shutdown);
        if let Some(h) = self.join.take() {
            let _ = h.join();
        }
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

    drain: Vec<Complex32>,
    audio_scratch: Vec<f32>,
    latest: Vec<f32>,
    fft_size: usize,

    last_data: Instant,
    reconnect_attempt: u32,
    retry_at: Option<Instant>,
    rate_window_start: Instant,
    rate_window_count: u64,
    cached_rate: f32,
    slow_since: Option<Instant>,
    running: bool,
}

impl Engine {
    fn new(
        cmd_rx: Receiver<EngineCommand>,
        shared: Arc<Mutex<EngineShared>>,
        params: Arc<Mutex<EngineParams>>,
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
            drain: Vec::with_capacity(MAX_DRAIN),
            audio_scratch: Vec::new(),
            latest: vec![-120.0; FFT_SIZE],
            fft_size: FFT_SIZE,
            last_data: Instant::now(),
            reconnect_attempt: 0,
            retry_at: None,
            rate_window_start: Instant::now(),
            rate_window_count: 0,
            cached_rate: 0.0,
            slow_since: None,
            running: true,
        }
    }

    fn run(&mut self) {
        while self.running {
            self.drain_commands();
            if !self.running {
                break;
            }

            let streaming = self.conn.is_some();
            if streaming {
                let got = self.pump_stream();
                self.maybe_reconnect_on_stall();
                if got == 0 {
                    thread::sleep(Duration::from_millis(3));
                }
            } else {
                self.maybe_retry_reconnect();
                thread::sleep(Duration::from_millis(20));
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
            EngineCommand::SetAudioDevice(name) => {
                self.audio_device = name;
                self.reopen_audio();
            }
            EngineCommand::Shutdown => {
                self.teardown();
                self.running = false;
            }
        }
    }

    fn start_connect(&mut self, req: &ConnectRequest) {
        self.teardown();
        self.set_state(ConnState::Connecting { label: req.label() });
        match connect(req) {
            Ok(conn) => {
                self.demod = IqAudioDemod::new();
                self.audio_device_open(conn.sample_rate as u32);
                self.last_data = Instant::now();
                self.rate_window_start = Instant::now();
                self.rate_window_count = 0;
                self.reconnect_attempt = 0;
                self.retry_at = None;
                self.slow_since = None;
                self.conn = Some(conn);
                self.set_error(None);
                self.set_state(ConnState::Streaming);
                self.publish_stats(0);
            }
            Err(error) => {
                self.set_error(Some(error));
                // Auto-reconnect with backoff; the UI still offers Disconnect.
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
    }

    fn audio_device_open(&mut self, source_rate: u32) {
        self.audio = match &self.audio_device {
            Some(name) => AudioOutput::try_open_named(name, source_rate)
                .or_else(|| AudioOutput::try_open_default(source_rate)),
            None => AudioOutput::try_open_default(source_rate),
        };
    }

    fn reopen_audio(&mut self) {
        let sr = self
            .conn
            .as_ref()
            .map(|c| c.sample_rate as u32)
            .unwrap_or(48_000);
        self.audio_device_open(sr);
    }

    /// Drain and process available IQ; returns sample count processed.
    fn pump_stream(&mut self) -> usize {
        let params = self.params.lock().map(|g| g.clone()).unwrap_or_default();

        if params.fft_size >= 256 && params.fft_size != self.fft_size {
            self.fft_size = params.fft_size;
            self.analyzer = SpectrumAnalyzer::new(self.fft_size, self.fft_size / 2);
            self.latest = vec![-120.0; self.fft_size];
        }

        self.drain.clear();
        if let Some(conn) = &mut self.conn {
            while self.drain.len() < MAX_DRAIN {
                match conn.iq.pop() {
                    Ok(s) => self.drain.push(s),
                    Err(_) => break,
                }
            }
        }
        let got = self.drain.len();
        if got == 0 {
            self.publish_stats(0);
            return 0;
        }
        self.last_data = Instant::now();
        self.rate_window_count += got as u64;

        let sample_rate = self.conn.as_ref().map(|c| c.sample_rate).unwrap_or(12_000.0);
        let center_hz = self.conn.as_ref().map(|c| c.center_hz).unwrap_or(0.0);

        if params.audio_enabled {
            self.demod
                .process(&self.drain, sample_rate, &params.cw, &mut self.audio_scratch);
            if let Some(audio) = &mut self.audio {
                audio.push(&self.audio_scratch, sample_rate as u32, params.volume);
            }
        }

        // FFT -> publish rows.
        let analyzer = &mut self.analyzer;
        let latest = &mut self.latest;
        let mut produced: Vec<Vec<f32>> = Vec::new();
        analyzer.process(&self.drain, |row| {
            latest.copy_from_slice(row);
            produced.push(row.to_vec());
        });

        self.skimmer.set_enabled(params.skimmer_enabled);
        if params.skimmer_enabled {
            self.skimmer
                .submit(&self.drain, &self.latest, sample_rate, center_hz);
        }

        let snr = self.demod.snr_db();
        self.publish_rows(produced, snr, got);
        got
    }

    fn publish_rows(&mut self, rows: Vec<Vec<f32>>, snr: f32, got: usize) {
        let spots = self.skimmer.spots();
        let channels = self.skimmer.active_channels();
        let dropped = self.conn.as_ref().map(|c| c.source.dropped_samples()).unwrap_or(0);
        let rssi = self.conn.as_ref().and_then(|c| c.source.rssi_dbm());
        let (sample_rate, is_kiwi) = self
            .conn
            .as_ref()
            .map(|c| (c.sample_rate, c.is_kiwi))
            .unwrap_or((12_000.0, false));
        let effective = self.effective_rate(sample_rate);
        let slow = self.update_slow_flag(sample_rate, effective);
        let (audio_device, audio_rate) = self
            .audio
            .as_ref()
            .map(|a| (Some(a.device_name().to_string()), a.output_rate()))
            .unwrap_or((None, 0));

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
                sample_rate,
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
            };
        }
    }

    fn publish_stats(&mut self, got: usize) {
        let dropped = self.conn.as_ref().map(|c| c.source.dropped_samples()).unwrap_or(0);
        let rssi = self.conn.as_ref().and_then(|c| c.source.rssi_dbm());
        let (sample_rate, is_kiwi) = self
            .conn
            .as_ref()
            .map(|c| (c.sample_rate, c.is_kiwi))
            .unwrap_or((12_000.0, false));
        let effective = self.effective_rate(sample_rate);
        let slow = self.update_slow_flag(sample_rate, effective);
        let (audio_device, audio_rate) = self
            .audio
            .as_ref()
            .map(|a| (Some(a.device_name().to_string()), a.output_rate()))
            .unwrap_or((None, 0));
        if let Ok(mut guard) = self.shared.lock() {
            guard.stats = EngineStats {
                sample_rate,
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
            };
        }
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
        if self.conn.is_none() {
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

    fn maybe_reconnect_on_stall(&mut self) {
        let link_error = self.conn.as_ref().and_then(|c| c.source.link_error());
        let stalled = self.last_data.elapsed() > STALL_TIMEOUT;
        if link_error.is_some() || stalled {
            let reason = link_error.unwrap_or_else(|| "connection stalled (no data)".to_string());
            self.teardown();
            self.set_error(Some(reason));
            self.schedule_reconnect();
            self.set_state(ConnState::Reconnecting {
                attempt: self.reconnect_attempt,
                retry_in_s: self.retry_secs(),
            });
        }
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
        let secs = self.retry_secs();
        self.retry_at = Some(Instant::now() + Duration::from_secs_f32(secs));
    }

    fn retry_secs(&self) -> f32 {
        let exp = self.reconnect_attempt.saturating_sub(1).min(5);
        (2u32.pow(exp) as f32).min(30.0)
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
