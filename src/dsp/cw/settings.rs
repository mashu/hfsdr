//! Per-signal CW listen settings — a toggleable, stackable filter pipeline.
//!
//! Each optional stage is its own sub-struct with an `enabled` flag and its
//! parameters. The channel runs them in a fixed, well-defined order; a future
//! node-graph compositor can reorder/connect these same stages without changing
//! the DSP structs themselves.

use super::fir::WindowKind;

/// Fixed number of independent manual notches (pileups have several hets).
pub const MAX_NOTCHES: usize = 4;

/// One steerable manual notch.
#[derive(Clone, Copy, Debug)]
pub struct NotchSpec {
    pub enabled: bool,
    pub offset_hz: f32,
    pub width_hz: f32,
}

impl Default for NotchSpec {
    fn default() -> Self {
        Self {
            enabled: false,
            offset_hz: 0.0,
            width_hz: 50.0,
        }
    }
}

/// Impulse noise blanker (wideband IQ, pre-channel).
#[derive(Clone, Copy, Debug)]
pub struct NoiseBlankerSettings {
    pub enabled: bool,
    pub threshold: f32,
    pub width: usize,
}

impl Default for NoiseBlankerSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            threshold: 6.0,
            width: 6,
        }
    }
}

/// Spotlight-aware LMS auto-notch.
#[derive(Clone, Copy, Debug)]
pub struct AutoNotchSettings {
    pub enabled: bool,
    pub guard_hz: f32,
    pub rate: f32,
}

impl Default for AutoNotchSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            guard_hz: 120.0,
            rate: 0.02,
        }
    }
}

/// Audio peak filter (resonant boost at pitch).
#[derive(Clone, Copy, Debug)]
pub struct ApfSettings {
    pub enabled: bool,
    pub width_hz: f32,
    pub gain: f32,
}

impl Default for ApfSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            width_hz: 120.0,
            gain: 1.5,
        }
    }
}

/// LMS line-enhancer noise reduction.
#[derive(Clone, Copy, Debug)]
pub struct NoiseReductionSettings {
    pub enabled: bool,
    pub level: f32,
}

impl Default for NoiseReductionSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            level: 0.3,
        }
    }
}

/// CW AGC, or manual gain when disabled.
#[derive(Clone, Copy, Debug)]
pub struct AgcSettings {
    pub enabled: bool,
    pub target: f32,
    pub attack_ms: f32,
    pub decay_ms: f32,
    pub manual_gain: f32,
}

impl Default for AgcSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            target: 0.25,
            attack_ms: 3.0,
            decay_ms: 120.0,
            manual_gain: 1.0,
        }
    }
}

/// Complete listen-chain configuration for one CW slice (VFO).
#[derive(Clone, Debug)]
pub struct CwChannelSettings {
    /// RF offset from hardware tune to the signal (Hz); RIT folds in here.
    pub listen_offset_hz: f32,
    pub bfo_hz: f32,
    pub passband_hz: f32,
    pub window: WindowKind,
    /// Kaiser β when `window == Kaiser` (typical 4–10).
    pub kaiser_beta: f32,
    /// Lift upstream SDR passband droop (inverse-sinc EQ convolved into channel FIR).
    pub passband_flatten: bool,
    /// Integer decimation factor override; `0` auto-selects from the IQ rate.
    pub decimation: u32,
    pub noise_blanker: NoiseBlankerSettings,
    pub notches: [NotchSpec; MAX_NOTCHES],
    pub auto_notch: AutoNotchSettings,
    pub apf: ApfSettings,
    pub noise_reduction: NoiseReductionSettings,
    pub agc: AgcSettings,
}

impl Default for CwChannelSettings {
    fn default() -> Self {
        Self {
            listen_offset_hz: 0.0,
            bfo_hz: 650.0,
            passband_hz: 200.0,
            window: WindowKind::Gaussian,
            kaiser_beta: 6.0,
            passband_flatten: false,
            decimation: 0,
            noise_blanker: NoiseBlankerSettings::default(),
            notches: [NotchSpec::default(); MAX_NOTCHES],
            auto_notch: AutoNotchSettings::default(),
            apf: ApfSettings::default(),
            noise_reduction: NoiseReductionSettings::default(),
            agc: AgcSettings::default(),
        }
    }
}

impl CwChannelSettings {
    pub fn channel_bandwidth_hz(&self) -> f32 {
        self.passband_hz.clamp(50.0, 2_000.0)
    }
}
