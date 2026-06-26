//! Per-signal CW listen settings — a toggleable, stackable filter pipeline.
//!
//! Each optional stage is its own sub-struct with an `enabled` flag and its
//! parameters. The channel runs them in a fixed, well-defined order; a future
//! node-graph compositor can reorder/connect these same stages without changing
//! the DSP structs themselves.

use super::filter_plan::{clamp_passband_hz, DEFAULT_CHANNEL_PASSBAND_HZ, DEFAULT_KAISER_BETA};
use super::fir::WindowKind;
use super::super::freq_offset::ChannelOffsetHz;

/// Default channel FIR window for new sessions.
pub const DEFAULT_CHANNEL_WINDOW: WindowKind = WindowKind::Blackman;

/// Fixed number of independent manual notches (pileups have several hets).
pub const MAX_NOTCHES: usize = 4;

/// One steerable manual notch.
#[derive(Clone, Copy, Debug)]
pub struct NotchSpec {
    pub enabled: bool,
    /// Panadapter / waterfall position (channel coordinates).
    pub offset_hz: ChannelOffsetHz,
    pub width_hz: f32,
}

impl Default for NotchSpec {
    fn default() -> Self {
        Self {
            enabled: false,
            offset_hz: ChannelOffsetHz::ZERO,
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

/// Channel selectivity implementation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChannelFilterKind {
    /// Windowed-sinc FIR (linear phase, default for CW keying).
    #[default]
    LinearFir,
    /// 2-pole biquad lowpass per rail (steeper, may ring).
    Iir2Pole,
}

/// Anti-alias filter for decimators / ingress (same implementations as channel).
pub type DecimFilterKind = ChannelFilterKind;

/// AGC gain law.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AgcMode {
    /// Symmetric envelope follower (current behaviour).
    #[default]
    Envelope,
    /// Fast gain reduction, slow recovery — less lift between dits (contest-style hang).
    Hang,
    /// Fast peak + slow floor trackers — RF/IF-style dual loop for CW.
    DualLoop,
}

/// Session-only A/B bypass flags (not persisted to settings.json).
///
/// Skipping a stage changes what you hear — use only to judge that stage's effect.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DiagnosticBypassSettings {
    /// Skip listen NCO (and wideband ingress mix). Hear hardware center, not RIT/tune.
    pub listen_nco: bool,
    /// Skip anti-alias FIR inside decimators (boxcar decimation; may alias).
    pub decim_fir: bool,
    /// Skip channel FIR (full IQ passband hits demod).
    pub channel_fir: bool,
    /// Skip BFO product detector (emit I-channel only, no pitch tone).
    pub bfo: bool,
}

impl DiagnosticBypassSettings {
    pub fn any_active(self) -> bool {
        self.listen_nco || self.decim_fir || self.channel_fir || self.bfo
    }
}

/// Complete listen-chain configuration for one CW slice (VFO).
#[derive(Clone, Debug)]
pub struct CwChannelSettings {
    /// RF offset from hardware tune to the signal (channel coordinates); RIT folds in here.
    pub listen_offset_hz: ChannelOffsetHz,
    /// Channel bandpass center in panadapter coordinates (independent of [`Self::listen_offset_hz`]).
    pub filter_offset_hz: ChannelOffsetHz,
    pub bfo_hz: f32,
    pub passband_hz: f32,
    pub channel_filter: ChannelFilterKind,
    /// Anti-alias filter on IQ decimators (channel + wideband ingress).
    pub decim_filter: DecimFilterKind,
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
    pub agc_mode: AgcMode,
    /// Diagnostic bypass (flow diagram / A/B); not saved to disk.
    pub diagnostic: DiagnosticBypassSettings,
    /// Use 2-pole IIR instead of linear FIR for lower CPU (may ring on fast CW).
    pub economy_filter: bool,
    /// Process the full IQ drain through listen demod (no tail cap on catch-up).
    pub full_demod: bool,
}

impl Default for CwChannelSettings {
    fn default() -> Self {
        Self {
            listen_offset_hz: ChannelOffsetHz::ZERO,
            filter_offset_hz: ChannelOffsetHz::ZERO,
            bfo_hz: 500.0,
            passband_hz: DEFAULT_CHANNEL_PASSBAND_HZ,
            channel_filter: ChannelFilterKind::LinearFir,
            decim_filter: DecimFilterKind::LinearFir,
            window: DEFAULT_CHANNEL_WINDOW,
            kaiser_beta: DEFAULT_KAISER_BETA,
            passband_flatten: false,
            decimation: 0,
            noise_blanker: NoiseBlankerSettings::default(),
            notches: [NotchSpec::default(); MAX_NOTCHES],
            auto_notch: AutoNotchSettings::default(),
            apf: ApfSettings::default(),
            noise_reduction: NoiseReductionSettings::default(),
            agc: AgcSettings::default(),
            agc_mode: AgcMode::Envelope,
            diagnostic: DiagnosticBypassSettings::default(),
            economy_filter: false,
            full_demod: true,
        }
    }
}

impl CwChannelSettings {
    /// Channel filter after economy override.
    pub fn effective_channel_filter(&self) -> ChannelFilterKind {
        if self.economy_filter {
            ChannelFilterKind::Iir2Pole
        } else {
            self.channel_filter
        }
    }

    pub fn channel_bandwidth_hz(&self) -> f32 {
        clamp_passband_hz(self.passband_hz)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::filter_plan::{CHANNEL_PASSBAND_MAX_HZ, CHANNEL_PASSBAND_MIN_HZ};

    #[test]
    fn defaults_match_contest_cw_profile() {
        let s = CwChannelSettings::default();
        assert_eq!(s.bfo_hz, 500.0);
        assert_eq!(s.window, DEFAULT_CHANNEL_WINDOW);
        assert_eq!(s.channel_filter, ChannelFilterKind::LinearFir);
        assert_eq!(s.agc_mode, AgcMode::Envelope);
        assert!(s.agc.enabled);
        assert!(s.full_demod);
        assert!(!s.diagnostic.any_active());
    }

    #[test]
    fn diagnostic_any_active() {
        assert!(!DiagnosticBypassSettings::default().any_active());
        assert!(DiagnosticBypassSettings {
            bfo: true,
            ..Default::default()
        }
        .any_active());
        assert!(DiagnosticBypassSettings {
            listen_nco: true,
            ..Default::default()
        }
        .any_active());
    }

    #[test]
    fn channel_bandwidth_clamps() {
        let narrow = CwChannelSettings {
            passband_hz: CHANNEL_PASSBAND_MIN_HZ,
            ..Default::default()
        };
        assert_eq!(narrow.channel_bandwidth_hz(), CHANNEL_PASSBAND_MIN_HZ);
        let wide = CwChannelSettings {
            passband_hz: 5_000.0,
            ..Default::default()
        };
        assert_eq!(wide.channel_bandwidth_hz(), CHANNEL_PASSBAND_MAX_HZ);
    }
}
