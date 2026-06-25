//! In-band CW skimmer scaffolding and shared spectral tools.

pub mod adaptive;
pub mod bigram;
pub mod config;
pub mod decoder;
pub mod engine;
pub mod envelope;
pub mod morse;
pub mod patterns;
pub mod peaks;
pub mod scp;
pub mod spots;

pub use adaptive::AdaptiveCwDecoder;
pub use bigram::BigramCwDecoder;
pub use config::{
    DecoderParams, EnvelopeSettings, SkimmerConfig, SkimmerDecoderKind,
};
pub use decoder::{dot_seconds_from_wpm, wpm_from_dot_seconds, CwDecoder};
pub use engine::Skimmer;
pub use morse::{decode_elements, encode_char};
pub use patterns::{analyze, looks_like_callsign, PatternMatch};
pub use scp::{MasterScp, MASTER_SCP_URL};
pub use peaks::{
    bin_to_offset_hz, detect_peaks, detect_peaks_with_floor, noise_floor_db, noise_floor_db_into,
    offset_hz_to_bin, strongest_offset_hz, strongest_offset_hz_with_floor, Peak,
};
pub use spots::{Spot, SpotKind, SpotSort, SpotStore};
