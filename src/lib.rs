//! # hfsdr — HF SDR / CW client library
//!
//! Source-agnostic IQ pipeline: panadapter FFT, contest-grade CW demodulation, in-band skimmer.
//!
//! ## Documentation (read this first)
//!
//! The **[mdBook](https://github.com/mashu/hfsdr/tree/main/docs)** explains behavior and
//! algorithms for operators and contributors — IQ basics, filter shapes, CW demod, skimmer,
//! and why the UI stays responsive. Build locally: `./scripts/build-docs.sh`.
//!
//! `cargo doc` is the API reference (types and functions), not a substitute for the book.
//!
//! ## Architecture in one diagram
//!
//! ```text
//! IqSource → ring → engine { listen (CwChannel) | FFT | skimmer } → GUI try_poll
//! ```

#[cfg(feature = "airspy")]
pub mod airspyhf;
#[cfg(feature = "rtlsdr")]
pub mod rtlsdr;
pub mod cty;
pub mod dsp;
pub mod history;
pub mod iq_record;
pub mod kiwi;
pub mod multisource;
pub mod skimmer;
pub mod source;

#[cfg(feature = "airspy")]
pub use airspyhf::AirspyHf;
#[cfg(feature = "rtlsdr")]
pub use rtlsdr::RtlSdr;
pub use cty::{Continent, ContinentResolver};
pub use history::{Annotation, RowFold, SlowWaterfall};
pub use iq_record::{default_capture_dir, read_meta, timestamped_capture_path, timestamped_capture_path_in, IqCaptureMeta, IqPlayback, IqRecorder};
pub use dsp::{
    auto_fft_size, bin_width_hz, channel_group_delay_ms, decimation_factor, design_gaussian_lowpass,
    design_lowpass,
    effective_decimation, audio_sample_rate, compose_panadapter_row, fit_panadapter_row_width,
    extract_passband_view, extract_view_window, panadapter_output_bins, iq_to_audio, spectrum_plan, spectrum_view_mapping,
    waterfall_storage_mapping, waterfall_storage_span_hz, waterfall_texture_u_range,
    view_t_to_offset_hz, offset_hz_to_view_t, offset_hz_to_storage_u, stretch_row_to_width,
    AgcSettings, ApfSettings, AutoNotchSettings, CwChannel, CwChannelSettings, FirDecimator,
    IngressWorker, IqAudioDemod,
    NoiseBlankerSettings, NoiseReductionSettings, NotchSpec, SpectrumAnalyzer, SpectrumFrontEnd,
    SpectrumViewMapping, WindowKind, MAX_FFT_SIZE, MAX_NOTCHES, TARGET_BIN_HZ,
    ZOOM_DECIM_THRESHOLD, spectrum_hop,
};
pub use kiwi::protocol::{kiwi_iq_half_hz, KIWI_IQ_HALF_HZ, KIWI_IQ_RATE};
pub use kiwi::KiwiSource;
pub use skimmer::{
    detect_peaks, detect_peaks_with_floor, strongest_offset_hz, strongest_offset_hz_with_floor,
    noise_floor_db, noise_floor_db_into, AdaptiveCwDecoder, BigramCwDecoder, CwDecoder, DecoderParams,
    EnvelopeSettings, MasterScp, Peak, Skimmer, SkimmerConfig, SkimmerDecoderKind, Spot,
    SpotKind, SpotSort, SpotStore, MASTER_SCP_URL,
};
pub use source::{Complex32, Consumer, IqSource, Result, SourceError};
