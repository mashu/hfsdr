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
//! IqSource → ring(s) → engine { listen (CwChannel) | FFT | skimmer } → GUI try_poll
//! ```
//!
//! When ingress decimation is configured, a bridge thread fans device IQ into raw +
//! decimated rings before the engine pump.

pub mod log;
pub mod native_sdr;
pub mod sdr_ffi;
#[cfg(feature = "soapy")]
pub mod soapy;
#[cfg(feature = "airspy")]
pub mod airspyhf;
#[cfg(feature = "rtlsdr")]
pub mod rtlsdr;
#[cfg(feature = "qmx")]
pub mod qmx;
pub mod cty;
pub mod dsp;
pub mod history;
pub mod iq_record;
pub mod kiwi;
pub mod pipeline_metrics;
pub mod multisource;
pub use pipeline_metrics::PipelineMetrics;
pub use multisource::{select_best, snr_weights, spot_display_snr, spot_primary_source, SourceSnr};
pub mod skimmer;
pub mod source;

#[cfg(any(test, coverage, mock_hal))]
pub mod mock_hal;

#[cfg(feature = "airspy")]
pub use airspyhf::AirspyHf;
#[cfg(feature = "rtlsdr")]
pub use rtlsdr::RtlSdr;
#[cfg(feature = "soapy")]
pub use soapy::SoapySource;
#[cfg(feature = "qmx")]
pub use qmx::QmxSource;
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
    BasebandOffsetHz, ChannelOffsetHz, ListenOrigin,
    AgcMode, AgcSettings, ApfSettings, AutoNotchSettings,     build_filter_overlay,
    build_listen_filter_curves, channel_magnitude_db_at, filter_curve_span_hz, ChannelFilterKind, CwChannel, CwChannelSettings, CwDetectorMode, CwSideband, CwStageMetrics,
    IirFilterKind, iir_2pole_lowpass_q, DEFAULT_IIR_CHEBYSHEV_RIPPLE_DB,
    DecimFilterKind, FirDecimator, FilterCurve, FilterCurveRequest, FilterOverlay, fir_cutoff_hz,
    gui_passband_edge_hz, notch_width_for_display_half, passband_hz_for_channel_half,
    channel_half_width_hz, filter_overlay_cache_key, OVERLAY_ATTEN_DB, plan_num_taps,
    IngressWorker, IqAudioDemod, IqApfSettings, IqWienerSettings, SquelchSettings, WidebandCwIngress,
    NoiseBlankerSettings, NoiseReductionSettings, NotchSpec, SidetoneEnvelope,
    SidetoneEnvelopeSettings, SidetoneEnvelopeShape, SpectrumAnalyzer, SpectrumFrontEnd,
    SpectrumViewMapping, WindowKind, FftWindowKind, DEFAULT_FFT_WINDOW, MAX_FFT_SIZE, MAX_NOTCHES,
    TARGET_BIN_HZ,
    ZOOM_DECIM_THRESHOLD, spectrum_hop, CHANNEL_PASSBAND_MAX_HZ, CHANNEL_PASSBAND_MIN_HZ,
    CHANNEL_PASSBAND_NARROW_MAX_HZ, DEFAULT_CHANNEL_PASSBAND_HZ, DEFAULT_CHANNEL_WINDOW,
    DEFAULT_DOLPH_SIDELOBE_DB, DEFAULT_KAISER_BETA, DEFAULT_PASSBAND_CUTOFF_FRAC,
    DEEP_SELECTIVITY_MAX_GROUP_DELAY_MS, MAX_DOLPH_SIDELOBE_DB, MAX_KAISER_BETA,
    MAX_PASSBAND_CUTOFF_FRAC, MIN_DOLPH_SIDELOBE_DB, MIN_KAISER_BETA, MIN_PASSBAND_CUTOFF_FRAC,
    PASSBAND_STEP_HZ, dit_duration_s, dit_samples, passband_hz_for_wpm,
};
pub use kiwi::protocol::{kiwi_iq_half_hz, KIWI_IQ_HALF_HZ, KIWI_IQ_RATE};
pub use kiwi::KiwiSource;
pub use skimmer::{
    detect_peaks, detect_peaks_with_floor, strongest_offset_hz, strongest_offset_hz_with_floor,
    encode_char, noise_floor_db, noise_floor_db_into, AdaptiveCwDecoder, BigramCwDecoder,
    CwDecoder, DecodeChannel, DecoderParams, EnvelopeSettings, MasterScp, Peak, Skimmer, SkimmerConfig,
    SkimmerDecoderKind, Spot, SpotKind, SpotSort, SpotStore, MASTER_SCP_URL,
};
pub use source::ingress::{
    effective_iq_process_hz, ingress_decimation_from_hz, DEFAULT_WIDEBAND_PROCESS_HZ,
    WIDEBAND_PROCESS_THRESHOLD_HZ,
};
pub use source::controls::*;
pub use source::{Complex32, Consumer, IqSource, Result, SourceError};
