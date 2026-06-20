//! Baseband audio from IQ for speaker output (CW / narrowband IQ).

use crate::source::Complex32;

/// Demodulate centered baseband IQ to mono audio (I-channel / real part).
///
/// Kiwi `mod=iq` with a narrow passband already delivers band-limited CW at DC;
/// the real component is the usual quick listen path before a full product detector.
pub fn iq_to_audio(samples: &[Complex32]) -> Vec<f32> {
    samples.iter().map(|s| s.re).collect()
}
