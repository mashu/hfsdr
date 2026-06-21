//! DSP building blocks for the panadapter and CW receiver paths.

mod audio;
mod biquad;
mod cw;
mod spectrum;
mod spectrum_front;
mod spectrum_plan;
mod view;

pub use audio::{iq_to_audio, IqAudioDemod};
pub use cw::{
    decimation_factor, design_gaussian_lowpass, design_lowpass, effective_decimation,
    audio_sample_rate, AgcSettings, ApfSettings,
    AudioPeakFilter, AutoNotch, AutoNotchSettings, ComplexNco, CwAgc, CwChannel, CwChannelSettings,
    Decimator, FirFilter, IqNotch, LmsPredictor, LmsStep, NoiseBlanker, NoiseBlankerSettings,
    NoiseReduction, NoiseReductionSettings, NotchSpec, ProductDetector, WindowKind, MAX_NOTCHES,
};
pub use spectrum::SpectrumAnalyzer;
pub use spectrum_front::SpectrumFrontEnd;
pub use spectrum_plan::{
    auto_fft_size, bin_width_hz, spectrum_plan, spectrum_zoom_decimation, MAX_FFT_SIZE,
    TARGET_BIN_HZ, ZOOM_DECIM_THRESHOLD,
};
pub use view::{extract_passband_view, extract_view_window, spectrum_view_mapping, SpectrumViewMapping};
