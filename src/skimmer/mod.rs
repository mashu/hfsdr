//! In-band CW skimmer scaffolding (build-order item 3) and shared spectral tools.
//!
//! Implemented today:
//! - [`peaks`] — peak picking + noise-floor/SNR estimation (also drives the
//!   receiver's zero-beat and pitch-lock features).
//! - [`spots`] — the spot store / contest dashboard model, with per-source SNR.
//! - [`scp`] — MASTER.SCP callsign dictionary (super check partial).
//! - [`decoder`] — the [`CwDecoder`] trait, the seam for the decoder bank.

pub mod adaptive;
pub mod bigram;
pub mod decoder;
pub mod engine;
pub mod morse;
pub mod patterns;
pub mod peaks;
pub mod scp;
pub mod spots;

pub use adaptive::AdaptiveCwDecoder;
pub use bigram::BigramCwDecoder;
pub use decoder::{dot_seconds_from_wpm, wpm_from_dot_seconds, CwDecoder};
pub use engine::{Skimmer, SkimmerConfig};
pub use morse::{decode_elements, encode_char};
pub use patterns::{analyze, looks_like_callsign, PatternMatch};
pub use scp::{MasterScp, MASTER_SCP_URL};
pub use peaks::{
    bin_to_offset_hz, detect_peaks, noise_floor_db, offset_hz_to_bin, strongest_offset_hz, Peak,
};
pub use spots::{Spot, SpotKind, SpotSort, SpotStore};
