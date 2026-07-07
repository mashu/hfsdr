//! Live per-channel CW decode state for the skimmer UI.

/// One active decoder channel (offset from RX center).
#[derive(Clone, Debug, PartialEq)]
pub struct DecodeChannel {
    pub offset_hz: f32,
    pub frequency_hz: f64,
    pub text: String,
    pub snr_db: f32,
    pub wpm: f32,
    /// Envelope gate is armed — real keying detected recently.
    pub keyed: bool,
}
