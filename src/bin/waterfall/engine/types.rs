//! Engine types shared with the UI thread.

use std::collections::VecDeque;

use hfsdr::{CwChannelSettings, Spot};
use hfsdr::SkimmerConfig;

use crate::skimmer::ScpStatus;
use crate::source::ConnectRequest;

use crate::engine::policy::MIN_SPECTRUM_ROWS_WIDEBAND;
use super::{FFT_SIZE, WATERFALL_ROWS};


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
    /// Pre-AGC IQ level for S-meter when hardware RSSI is unavailable.
    pub iq_rf_level: f32,
    /// KiwiSDR 2 reports `has_attn=1` when a hardware RF attenuator is present.
    pub kiwi_has_rf_attn: bool,
    /// Latest Kiwi hardware RF attenuator setting (dB).
    pub kiwi_rf_attn_db: f32,
    /// Last hardware RF gain sent to the source (Kiwi `manGain`, etc.).
    pub hw_rf_gain: Option<u8>,
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
            iq_rf_level: 0.0,
            kiwi_has_rf_attn: false,
            kiwi_rf_attn_db: 0.0,
            hw_rf_gain: None,
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
    /// Yaesu-style software RF gain (dB) applied to IQ before spectrum, S-meter, and AGC.
    ///
    /// Source-independent: works on every radio, even when hardware/RF AGC is on,
    /// because it scales the IQ we receive rather than a front-end gain stage.
    pub rf_gain_db: f32,
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
            rf_gain_db: 0.0,
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
