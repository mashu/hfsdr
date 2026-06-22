//! DSP building blocks for the panadapter and CW receiver paths.
//!
//! - **Spectrum:** [`SpectrumAnalyzer`], [`SpectrumFrontEnd`], [`spectrum_plan`].
//! - **CW listen:** [`CwChannel`], [`CwChannelSettings`], and submodules under `cw/`.
//! - **View:** [`extract_view_window`] maps FFT bins to zoom/pan.
//!
//! See the mdBook chapters *Spectrum and waterfall* and *CW receive chain* in `docs/`.

mod audio;
mod biquad;
mod cw;
mod spectrum;
mod spectrum_front;
mod spectrum_plan;
mod view;

pub use audio::{iq_to_audio, IqAudioDemod};
pub use cw::{
    channel_group_delay_ms, decimation_factor, design_gaussian_lowpass, design_lowpass,
    design_lowpass_with, effective_decimation, audio_sample_rate, AgcSettings, ApfSettings,
    AudioPeakFilter, AutoNotch, AutoNotchSettings, ComplexNco, CwAgc, CwChannel, CwChannelSettings,
    Decimator, FirFilter, LowpassDesign, IqNotch, LmsPredictor, LmsStep, NoiseBlanker,
    NoiseBlankerSettings, NoiseReduction, NoiseReductionSettings, NotchSpec, ProductDetector,
    WindowKind, MAX_NOTCHES,
};
pub use spectrum::SpectrumAnalyzer;
pub use spectrum_front::SpectrumFrontEnd;
pub use spectrum_plan::{
    auto_fft_size, bin_width_hz, spectrum_plan, spectrum_zoom_decimation, MAX_FFT_SIZE,
    TARGET_BIN_HZ, ZOOM_DECIM_THRESHOLD,
};
pub use view::{
    compose_panadapter_row, downsample_row_peak, extract_passband_view, extract_view_window,
    fit_panadapter_row_width, panadapter_output_bins, spectrum_view_mapping, MAX_PANADAPTER_BINS, WIDE_PANADAPTER_BINS,
    SpectrumViewMapping,
};
