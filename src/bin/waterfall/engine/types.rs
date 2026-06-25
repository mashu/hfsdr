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

fn finite_or(value: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value
    } else {
        fallback
    }
}

fn sanitize_db_row(row: &mut Vec<f32>, fallback_len: usize) {
    if row.is_empty() {
        row.resize(fallback_len.max(1), -120.0);
        return;
    }
    for v in row.iter_mut() {
        if !v.is_finite() {
            *v = -120.0;
        }
    }
}

impl EngineStats {
    /// Clamp non-finite meter/plot inputs before the UI consumes them.
    pub fn sanitized(mut self) -> Self {
        self.sample_rate = finite_or(self.sample_rate, 12_000.0).max(1.0);
        self.iq_passband_hz = finite_or(self.iq_passband_hz, self.sample_rate).max(1.0);
        self.effective_sps = finite_or(self.effective_sps, 0.0).max(0.0);
        self.snr_db = finite_or(self.snr_db, 0.0);
        self.iq_rf_level = finite_or(self.iq_rf_level, 0.0).max(0.0);
        self.agc_gain = finite_or(self.agc_gain, 1.0).max(1e-6);
        self.agc_envelope = finite_or(self.agc_envelope, 0.0).max(0.0);
        self.audio_peak = finite_or(self.audio_peak, 0.0).max(0.0);
        self.audio_rms = finite_or(self.audio_rms, 0.0).max(0.0);
        self.iq_buffer_fill = finite_or(self.iq_buffer_fill, 0.0).clamp(0.0, 1.0);
        self.iq_buffer_secs = finite_or(self.iq_buffer_secs, 0.0).max(0.0);
        self.spectrum_rate = finite_or(self.spectrum_rate, self.sample_rate).max(1.0);
        self.kiwi_rf_attn_db = finite_or(self.kiwi_rf_attn_db, 0.0);
        if let Some(rssi) = self.rssi_dbm {
            self.rssi_dbm = Some(finite_or(rssi, -127.0));
        }
        self.spectrum_fft = self.spectrum_fft.max(1024);
        self
    }
}

impl EnginePoll {
    /// Replace NaN/Inf spectrum bins and bogus stats before UI ingest.
    pub fn sanitized(mut self, fft_fallback_len: usize) -> Self {
        self.stats = self.stats.sanitized();
        let row_len = self.latest.len().max(fft_fallback_len).max(FFT_SIZE);
        sanitize_db_row(&mut self.latest, row_len);
        for row in &mut self.rows {
            sanitize_db_row(row, self.latest.len());
        }
        for sample in &mut self.audio_scope {
            if !sample.is_finite() {
                *sample = 0.0;
            }
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_stats_replaces_nan() {
        let mut stats = EngineStats::default();
        stats.sample_rate = f32::NAN;
        stats.snr_db = f32::INFINITY;
        stats.iq_rf_level = f32::NAN;
        stats.agc_gain = f32::NAN;
        let stats = stats.sanitized();
        assert!(stats.sample_rate.is_finite() && stats.sample_rate > 0.0);
        assert!(stats.snr_db.is_finite());
        assert!(stats.iq_rf_level.is_finite());
        assert!(stats.agc_gain.is_finite() && stats.agc_gain > 0.0);
    }

    #[test]
    fn sanitize_poll_fills_empty_spectrum() {
        let poll = EnginePoll {
            state: ConnState::Streaming,
            stats: EngineStats::default(),
            spots: Vec::new(),
            rows: Vec::new(),
            latest: Vec::new(),
            last_error: None,
            audio_scope: vec![f32::NAN, 1.0],
        };
        let poll = poll.sanitized(FFT_SIZE);
        assert_eq!(poll.latest.len(), FFT_SIZE);
        assert!(poll.latest.iter().all(|v| v.is_finite()));
        assert!(poll.audio_scope.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn sanitize_poll_replaces_nan_bins() {
        let mut latest = vec![-90.0; 8];
        latest[3] = f32::NAN;
        latest[7] = f32::INFINITY;
        let poll = EnginePoll {
            state: ConnState::Streaming,
            stats: EngineStats::default(),
            spots: Vec::new(),
            rows: vec![latest.clone()],
            latest,
            last_error: None,
            audio_scope: Vec::new(),
        };
        let poll = poll.sanitized(8);
        assert!(poll.latest.iter().all(|v| v.is_finite()));
        assert!(poll.rows[0].iter().all(|v| v.is_finite()));
    }

    #[test]
    fn sanitize_poll_fills_empty_row_entries() {
        let poll = EnginePoll {
            state: ConnState::Streaming,
            stats: EngineStats::default(),
            spots: Vec::new(),
            rows: vec![vec![]],
            latest: vec![-90.0; 8],
            last_error: None,
            audio_scope: Vec::new(),
        };
        let poll = poll.sanitized(8);
        assert_eq!(poll.rows[0].len(), 8);
        assert!(poll.rows[0].iter().all(|v| *v <= -119.0));
    }

    #[test]
    fn sanitize_stats_clamps_buffer_fill() {
        let mut stats = EngineStats::default();
        stats.iq_buffer_fill = 2.5;
        stats.spectrum_fft = 512;
        let stats = stats.sanitized();
        assert!((stats.iq_buffer_fill - 1.0).abs() < 1e-6);
        assert!(stats.spectrum_fft >= 1024);
    }
}
