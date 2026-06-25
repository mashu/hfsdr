//! CW decoder interface for the skimmer's decoder bank.
//!
//! Each detected peak gets its own narrowband demod feeding a [`CwDecoder`].
//! The trait keeps the decode algorithm pluggable: [`super::adaptive::AdaptiveCwDecoder`]
//! for a lightweight baseline, [`super::bigram::BigramCwDecoder`] for beam-search
//! with a callsign-biased bigram model.

/// A decoder consuming one channel's audio and emitting decoded text.
pub trait CwDecoder: Send {
    /// Feed mono audio at `sample_rate`; returns any newly decoded characters.
    fn push_audio(&mut self, audio: &[f32], sample_rate: f32) -> String;

    /// Current adaptive speed estimate in WPM.
    fn wpm(&self) -> f32;

    /// Reset internal state (e.g. when retuned to a new peak).
    fn reset(&mut self);
}

/// Estimate WPM from a CW dot length in seconds (PARIS standard).
pub fn wpm_from_dot_seconds(dot_seconds: f32) -> f32 {
    if dot_seconds <= 0.0 {
        return 0.0;
    }
    1.2 / dot_seconds
}

/// Dot length in seconds for a given WPM.
pub fn dot_seconds_from_wpm(wpm: f32) -> f32 {
    if wpm <= 0.0 {
        return 0.0;
    }
    1.2 / wpm
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wpm_dot_roundtrip() {
        let dot = dot_seconds_from_wpm(25.0);
        assert!((wpm_from_dot_seconds(dot) - 25.0).abs() < 0.01);
    }
}
