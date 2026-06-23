//! CW contest receiver DSP — modular, toggleable per-signal listen chain.
//!
//! Pipeline (each optional stage bypassable, see [`channel::CwChannel`]):
//! noise blanker → NCO (RIT/listen offset) → decimate → manual notches →
//! Gaussian/raised-cosine channel filter → AGC → BFO product detector → APF →
//! auto-notch → noise reduction → audio.
//!
//! Every stage is a self-contained, single-responsibility struct so a future
//! node-graph compositor can wire them together visually. Heavier contest
//! features (skimmer, history, multi-source) live in sibling crates of modules:
//! [`crate::skimmer`].

mod agc;
mod apf;
mod autonotch;
mod channel;
mod decimator;
mod detector;
mod fir;
mod lms;
mod nco;
mod noiseblanker;
mod noisereduction;
mod notch;
mod settings;

pub use agc::CwAgc;
pub use apf::AudioPeakFilter;
pub use autonotch::AutoNotch;
pub use channel::CwChannel;
pub use decimator::{audio_sample_rate, decimation_factor, effective_decimation, Decimator};
pub use detector::ProductDetector;
pub use fir::{
    channel_group_delay_ms, design_gaussian_lowpass, design_gaussian_lowpass_compact,
    design_lowpass, design_lowpass_with, FirFilter, LowpassDesign, WindowKind,
};
pub use lms::{LmsPredictor, LmsStep};
pub use nco::ComplexNco;
pub use noiseblanker::NoiseBlanker;
pub use noisereduction::NoiseReduction;
pub use notch::IqNotch;
pub use settings::{
    AgcSettings, ApfSettings, AutoNotchSettings, CwChannelSettings, NoiseBlankerSettings,
    NoiseReductionSettings, NotchSpec, MAX_NOTCHES,
};
