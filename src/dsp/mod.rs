//! DSP building blocks for the panadapter and CW receiver paths.

mod audio;
mod biquad;
mod cw;
mod spectrum;
mod view;

pub use audio::{iq_to_audio, IqAudioDemod};
pub use cw::{
    decimation_factor, design_gaussian_lowpass, design_lowpass, AgcSettings, ApfSettings,
    AudioPeakFilter, AutoNotch, AutoNotchSettings, ComplexNco, CwAgc, CwChannel, CwChannelSettings,
    Decimator, FirFilter, IqNotch, LmsPredictor, LmsStep, NoiseBlanker, NoiseBlankerSettings,
    NoiseReduction, NoiseReductionSettings, NotchSpec, ProductDetector, WindowKind, MAX_NOTCHES,
};
pub use spectrum::SpectrumAnalyzer;
pub use view::{extract_passband_view, extract_view_window};
