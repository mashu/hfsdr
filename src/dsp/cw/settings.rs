//! Per-signal CW listen settings — a toggleable, stackable filter pipeline.
//!
//! Each optional stage is its own sub-struct with an `enabled` flag and its
//! parameters. The channel runs them in a fixed, well-defined order; a future
//! node-graph compositor can reorder/connect these same stages without changing
//! the DSP structs themselves.

use super::filter_plan::{
    clamp_passband_hz, DEFAULT_CHANNEL_PASSBAND_HZ, DEFAULT_DOLPH_SIDELOBE_DB,
    DEFAULT_KAISER_BETA, DEFAULT_PASSBAND_CUTOFF_FRAC, MAX_DOLPH_SIDELOBE_DB,
    MAX_PASSBAND_CUTOFF_FRAC, MIN_DOLPH_SIDELOBE_DB, MIN_PASSBAND_CUTOFF_FRAC,
};
use super::fir::{LowpassDesign, WindowKind};
use super::super::freq_offset::ChannelOffsetHz;

pub use super::sidetone_envelope::{SidetoneEnvelopeSettings, SidetoneEnvelopeShape};

/// CW product-detector sideband — which side of the carrier you zero-beat on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CwSideband {
    /// CW-L (LSB-style): tune **above** the signal; BFO mixes up.
    #[default]
    Lower,
    /// CW-U (USB-style): tune **below** the signal; BFO mixes down.
    Upper,
}

impl CwSideband {
    /// Mix direction for the BFO product detector (`Lower` → mix up, `Upper` → mix down).
    pub fn mix_up(self) -> bool {
        matches!(self, Self::Lower)
    }
}

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

pub use super::cw_detector::CwDetectorMode;

/// IQ-domain resonant peak before demod.
#[derive(Clone, Copy, Debug)]
pub struct IqApfSettings {
    pub enabled: bool,
    pub width_hz: f32,
    pub gain: f32,
}

impl Default for IqApfSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            width_hz: 60.0,
            gain: 1.2,
        }
    }
}

/// Pre-demod Wiener-style IQ noise suppression.
#[derive(Clone, Copy, Debug)]
pub struct IqWienerSettings {
    pub enabled: bool,
    pub level: f32,
}

impl Default for IqWienerSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            level: 0.28,
        }
    }
}

/// CW squelch with hang (post-demod).
#[derive(Clone, Copy, Debug)]
pub struct SquelchSettings {
    pub enabled: bool,
    pub open_threshold: f32,
    pub close_threshold: f32,
    pub hang_ms: f32,
    /// Gate open/close ramp (ms) — longer is softer, shorter is snappier.
    pub ramp_ms: f32,
}

impl Default for SquelchSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            open_threshold: 0.02,
            close_threshold: 0.01,
            hang_ms: 120.0,
            ramp_ms: super::squelch::DEFAULT_SQUELCH_RAMP_MS,
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
    /// Forward scan window for [`AgcMode::Lookahead`] only.
    pub lookahead_ms: f32,
    pub manual_gain: f32,
}

impl Default for AgcSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            target: 0.25,
            attack_ms: 3.0,
            decay_ms: 120.0,
            lookahead_ms: 8.0,
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

/// 2-pole IIR prototype — [`ChannelFilterKind::Iir2Pole`] only.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum IirFilterKind {
    /// Maximally flat passband (Q ≈ 0.707).
    #[default]
    Butterworth,
    /// Steeper stopband with controlled passband ripple (~2 dB).
    Chebyshev,
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
    /// Forward peak scan + slow gain ramps — pre-ducks before known peaks to limit overshoot.
    Lookahead,
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
    /// Bandpass center offset from [`Self::listen_offset_hz`] (0 = filter centered on VFO).
    pub filter_shift_hz: ChannelOffsetHz,
    pub bfo_hz: f32,
    /// CW-L vs CW-U product-detector sideband.
    pub sideband: CwSideband,
    pub passband_hz: f32,
    pub channel_filter: ChannelFilterKind,
    /// 2-pole prototype when [`Self::effective_channel_filter`] is IIR.
    pub iir_filter: IirFilterKind,
    /// Anti-alias filter on IQ decimators (channel + wideband ingress).
    pub decim_filter: DecimFilterKind,
    pub window: WindowKind,
    /// Kaiser β when `window == Kaiser` (typical 4–10).
    pub kaiser_beta: f32,
    /// Lift upstream SDR passband droop (inverse-sinc EQ convolved into channel FIR).
    pub passband_flatten: bool,
    /// Sinc cutoff as a fraction of GUI passband width (soft ↔ sharp).
    pub passband_cutoff_frac: f32,
    /// Use maximum group-delay budget for steeper FIR skirts.
    pub deep_selectivity: bool,
    /// Target sidelobe attenuation (dB) for Dolph–Chebyshev FIR.
    pub dolph_sidelobe_db: f32,
    /// CW demod strategy (product / coherent / dit-matched).
    pub detector_mode: CwDetectorMode,
    pub iq_apf: IqApfSettings,
    pub iq_wiener: IqWienerSettings,
    pub squelch: SquelchSettings,
    /// Integer decimation factor override; `0` auto-selects from the IQ rate.
    pub decimation: u32,
    pub noise_blanker: NoiseBlankerSettings,
    pub notches: [NotchSpec; MAX_NOTCHES],
    pub auto_notch: AutoNotchSettings,
    pub apf: ApfSettings,
    pub noise_reduction: NoiseReductionSettings,
    pub agc: AgcSettings,
    pub agc_mode: AgcMode,
    pub sidetone_envelope: SidetoneEnvelopeSettings,
    /// Diagnostic bypass (flow diagram / A/B); not saved to disk.
    pub diagnostic: DiagnosticBypassSettings,
    /// Process the full IQ drain through listen demod (no tail cap on catch-up).
    pub full_demod: bool,
}

impl Default for CwChannelSettings {
    fn default() -> Self {
        Self {
            listen_offset_hz: ChannelOffsetHz::ZERO,
            filter_shift_hz: ChannelOffsetHz::ZERO,
            bfo_hz: 500.0,
            sideband: CwSideband::Lower,
            passband_hz: DEFAULT_CHANNEL_PASSBAND_HZ,
            channel_filter: ChannelFilterKind::LinearFir,
            iir_filter: IirFilterKind::Butterworth,
            decim_filter: DecimFilterKind::LinearFir,
            window: DEFAULT_CHANNEL_WINDOW,
            kaiser_beta: DEFAULT_KAISER_BETA,
            passband_flatten: false,
            passband_cutoff_frac: DEFAULT_PASSBAND_CUTOFF_FRAC,
            deep_selectivity: false,
            dolph_sidelobe_db: DEFAULT_DOLPH_SIDELOBE_DB,
            detector_mode: CwDetectorMode::Product,
            iq_apf: IqApfSettings::default(),
            iq_wiener: IqWienerSettings::default(),
            squelch: SquelchSettings::default(),
            decimation: 0,
            noise_blanker: NoiseBlankerSettings::default(),
            notches: [NotchSpec::default(); MAX_NOTCHES],
            auto_notch: AutoNotchSettings::default(),
            apf: ApfSettings::default(),
            noise_reduction: NoiseReductionSettings::default(),
            agc: AgcSettings::default(),
            agc_mode: AgcMode::Envelope,
            sidetone_envelope: SidetoneEnvelopeSettings::default(),
            diagnostic: DiagnosticBypassSettings::default(),
            full_demod: true,
        }
    }
}

impl CwChannelSettings {
    /// Channel filter after legacy economy override (always [`Self::channel_filter`]).
    pub fn effective_channel_filter(&self) -> ChannelFilterKind {
        self.channel_filter
    }

    pub fn channel_bandwidth_hz(&self) -> f32 {
        clamp_passband_hz(self.passband_hz)
    }

    /// Build FIR design parameters from listen settings.
    pub fn lowpass_design(&self) -> LowpassDesign {
        LowpassDesign {
            window: self.window,
            kaiser_beta: self.kaiser_beta,
            passband_flatten: self.passband_flatten,
            cutoff_frac: self
                .passband_cutoff_frac
                .clamp(MIN_PASSBAND_CUTOFF_FRAC, MAX_PASSBAND_CUTOFF_FRAC),
            deep_selectivity: self.deep_selectivity,
            dolph_sidelobe_db: self
                .dolph_sidelobe_db
                .clamp(MIN_DOLPH_SIDELOBE_DB, MAX_DOLPH_SIDELOBE_DB),
        }
    }

    /// Group delay of the channel FIR at the current audio rate (ms).
    pub fn channel_group_delay_ms(&self, audio_rate: f32) -> f32 {
        super::filter_plan::channel_group_delay_ms(
            audio_rate,
            self.channel_bandwidth_hz(),
            self.passband_cutoff_frac,
            self.deep_selectivity,
        )
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
