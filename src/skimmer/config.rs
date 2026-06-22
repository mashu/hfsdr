//! Skimmer bank configuration — all tunables with documented defaults.

use std::time::Duration;

/// Morse decoder algorithm for each skimmer channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SkimmerDecoderKind {
    /// Beam search + callsign bigram model (best quality, more CPU).
    #[default]
    Bigram,
    /// Adaptive envelope FSM (lighter CPU).
    Adaptive,
}

/// Envelope keying thresholds as fractions of (peak − noise).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EnvelopeSettings {
    pub thr_low: f32,
    pub thr_high: f32,
    /// Minimum (peak − noise) / peak before treating energy as signal.
    pub min_span_fraction: f32,
}

impl Default for EnvelopeSettings {
    fn default() -> Self {
        Self {
            thr_low: 0.55,
            thr_high: 0.72,
            min_span_fraction: 0.10,
        }
    }
}

impl EnvelopeSettings {
    pub fn clamped(self) -> Self {
        let lo = self.thr_low.clamp(0.05, 0.95);
        let hi = self.thr_high.clamp(lo + 0.05, 0.99);
        Self {
            thr_low: lo,
            thr_high: hi,
            min_span_fraction: self.min_span_fraction.clamp(0.02, 0.40),
        }
    }
}

/// Per-decoder parameters (shared shape; beam width ignored by adaptive).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DecoderParams {
    pub initial_wpm: f32,
    pub beam_width: usize,
    pub envelope: EnvelopeSettings,
    pub max_text_chars: usize,
}

impl Default for DecoderParams {
    fn default() -> Self {
        Self {
            initial_wpm: 22.0,
            beam_width: 12,
            envelope: EnvelopeSettings::default(),
            max_text_chars: 64,
        }
    }
}

impl DecoderParams {
    pub fn clamped(self) -> Self {
        Self {
            initial_wpm: self.initial_wpm.clamp(8.0, 60.0),
            beam_width: self.beam_width.clamp(1, 64),
            envelope: self.envelope.clamped(),
            max_text_chars: self.max_text_chars.clamp(16, 256),
        }
    }
}

/// Full skimmer configuration.
#[derive(Clone, Debug, PartialEq)]
pub struct SkimmerConfig {
    pub bucket_hz: f32,
    pub min_snr_db: f32,
    /// Peaks below this SNR may exist but decoders stay muted.
    pub min_decode_snr_db: f32,
    pub min_separation_bins: usize,
    pub max_channels: usize,
    pub channel_timeout_secs: f32,
    pub spot_store_max_age_secs: f32,
    pub source_label: String,
    pub require_scp: bool,
    pub decoder: SkimmerDecoderKind,
    pub lpf_cutoff_hz: f32,
    pub target_audio_rate_hz: f32,
    /// Sustained keyed energy required before Morse decode (ms).
    pub decode_gate_ms: f32,
    pub decoder_params: DecoderParams,
}

impl Default for SkimmerConfig {
    fn default() -> Self {
        Self {
            bucket_hz: 80.0,
            min_snr_db: 10.0,
            min_decode_snr_db: 8.0,
            min_separation_bins: 5,
            max_channels: 16,
            channel_timeout_secs: 8.0,
            spot_store_max_age_secs: 120.0,
            source_label: "rx".to_string(),
            require_scp: false,
            decoder: SkimmerDecoderKind::Adaptive,
            lpf_cutoff_hz: 150.0,
            target_audio_rate_hz: 12_000.0,
            decode_gate_ms: 45.0,
            decoder_params: DecoderParams::default(),
        }
    }
}

impl SkimmerConfig {
    pub fn clamped(mut self) -> Self {
        self.bucket_hz = self.bucket_hz.clamp(20.0, 200.0);
        self.min_snr_db = self.min_snr_db.clamp(0.0, 60.0);
        self.min_decode_snr_db = self.min_decode_snr_db.clamp(self.min_snr_db, 60.0);
        self.min_separation_bins = self.min_separation_bins.clamp(1, 32);
        self.max_channels = self.max_channels.max(1);
        self.channel_timeout_secs = self.channel_timeout_secs.clamp(1.0, 120.0);
        self.spot_store_max_age_secs = self.spot_store_max_age_secs.max(0.0);
        self.lpf_cutoff_hz = self.lpf_cutoff_hz.clamp(40.0, 800.0);
        self.target_audio_rate_hz = self.target_audio_rate_hz.clamp(4_000.0, 48_000.0);
        self.decode_gate_ms = self.decode_gate_ms.clamp(20.0, 500.0);
        self.decoder_params = self.decoder_params.clamped();
        self
    }

    pub fn channel_timeout(&self) -> Duration {
        Duration::from_secs_f32(self.channel_timeout_secs)
    }

    pub fn spot_store_max_age(&self) -> Duration {
        if self.spot_store_max_age_secs <= 0.0 {
            Duration::from_secs(86400 * 7)
        } else {
            Duration::from_secs_f32(self.spot_store_max_age_secs)
        }
    }

    /// Changing these requires tearing down active decoder channels.
    pub fn channel_dsp_changed(&self, other: &Self) -> bool {
        self.decoder != other.decoder
            || (self.lpf_cutoff_hz - other.lpf_cutoff_hz).abs() > f32::EPSILON
            || (self.target_audio_rate_hz - other.target_audio_rate_hz).abs() > f32::EPSILON
            || (self.decode_gate_ms - other.decode_gate_ms).abs() > f32::EPSILON
            || (self.min_decode_snr_db - other.min_decode_snr_db).abs() > f32::EPSILON
            || self.decoder_params != other.decoder_params
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        let c = SkimmerConfig::default().clamped();
        assert_eq!(c.decoder, SkimmerDecoderKind::Adaptive);
        assert!(c.decoder_params.envelope.thr_high > c.decoder_params.envelope.thr_low);
    }

    #[test]
    fn channel_dsp_change_detects_decoder_switch() {
        let a = SkimmerConfig::default();
        let mut b = a.clone();
        b.decoder = SkimmerDecoderKind::Bigram;
        assert!(a.channel_dsp_changed(&b));
    }
}
