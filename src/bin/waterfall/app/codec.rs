//! Settings serialization helpers and small shared utilities.

use hfsdr::{
    AgcMode, ChannelFilterKind, SkimmerConfig, SkimmerDecoderKind, SpotSort, WindowKind,
    DecoderParams, EnvelopeSettings,
};

use crate::settings::AppSettings;
use crate::interaction::PlotAction;

pub(crate) fn plot_action_changes_view(action: &PlotAction) -> bool {
    matches!(
        action,
        PlotAction::PanViewDeltaHz(_) | PlotAction::ZoomView(_) | PlotAction::SetViewPanHz(_)
    )
}

pub(crate) const fn window_to_u8(w: WindowKind) -> u8 {
    match w {
        WindowKind::Gaussian => 0,
        WindowKind::RaisedCosine => 1,
        WindowKind::Blackman => 2,
        WindowKind::Kaiser => 3,
    }
}

pub(crate) fn window_from_u8(v: u8) -> WindowKind {
    match v {
        1 => WindowKind::RaisedCosine,
        2 => WindowKind::Blackman,
        3 => WindowKind::Kaiser,
        _ => WindowKind::Gaussian,
    }
}

pub(crate) fn channel_filter_to_u8(k: ChannelFilterKind) -> u8 {
    match k {
        ChannelFilterKind::LinearFir => 0,
        ChannelFilterKind::Iir2Pole => 1,
    }
}

pub(crate) fn channel_filter_from_u8(v: u8) -> ChannelFilterKind {
    match v {
        1 => ChannelFilterKind::Iir2Pole,
        _ => ChannelFilterKind::LinearFir,
    }
}

pub(crate) fn agc_mode_to_u8(m: AgcMode) -> u8 {
    match m {
        AgcMode::Envelope => 0,
        AgcMode::Hang => 1,
        AgcMode::DualLoop => 2,
    }
}

pub(crate) fn agc_mode_from_u8(v: u8) -> AgcMode {
    match v {
        1 => AgcMode::Hang,
        2 => AgcMode::DualLoop,
        _ => AgcMode::Envelope,
    }
}

pub(crate) fn spot_sort_to_u8(s: SpotSort) -> u8 {
    match s {
        SpotSort::SnrDesc => 0,
        SpotSort::Frequency => 1,
        SpotSort::LastHeard => 2,
        SpotSort::Callsign => 3,
    }
}

pub(crate) fn spot_sort_from_u8(v: u8) -> SpotSort {
    match v {
        1 => SpotSort::Frequency,
        2 => SpotSort::LastHeard,
        3 => SpotSort::Callsign,
        _ => SpotSort::SnrDesc,
    }
}

pub(crate) fn skimmer_decoder_to_u8(d: SkimmerDecoderKind) -> u8 {
    match d {
        SkimmerDecoderKind::Bigram => 0,
        SkimmerDecoderKind::Adaptive => 1,
    }
}

pub(crate) fn skimmer_decoder_from_u8(v: u8) -> SkimmerDecoderKind {
    match v {
        1 => SkimmerDecoderKind::Adaptive,
        _ => SkimmerDecoderKind::Bigram,
    }
}

pub(crate) fn skimmer_config_from_settings(s: &AppSettings) -> SkimmerConfig {
    use hfsdr::{DecoderParams, EnvelopeSettings};
    SkimmerConfig {
        bucket_hz: s.skimmer_bucket_hz,
        min_snr_db: s.skimmer_min_snr_db,
        min_decode_snr_db: s.skimmer_min_decode_snr_db,
        min_separation_bins: s.skimmer_min_separation_bins,
        max_channels: s.skimmer_max_channels.max(1),
        channel_timeout_secs: s.skimmer_channel_timeout_secs,
        spot_store_max_age_secs: s.skimmer_store_max_age_secs,
        source_label: "rx".to_string(),
        require_scp: s.scp_require,
        decoder: skimmer_decoder_from_u8(s.skimmer_decoder),
        lpf_cutoff_hz: s.skimmer_lpf_cutoff_hz,
        target_audio_rate_hz: s.skimmer_target_audio_rate_hz,
        decode_gate_ms: s.skimmer_decode_gate_ms,
        decoder_params: DecoderParams {
            initial_wpm: s.skimmer_initial_wpm,
            beam_width: s.skimmer_beam_width.max(1),
            envelope: EnvelopeSettings {
                thr_low: s.skimmer_thr_low,
                thr_high: s.skimmer_thr_high,
                min_span_fraction: EnvelopeSettings::default().min_span_fraction,
            },
            max_text_chars: s.skimmer_max_decode_chars.max(16),
        },
    }
    .clamped()
}

pub(crate) fn normalize_waterfall_avg(value: u8) -> u8 {
    match value {
        2 => 2,
        4 => 4,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interaction::PlotAction;
    use crate::settings::AppSettings;
    use hfsdr::{
        AgcMode, ChannelFilterKind, SkimmerDecoderKind, SpotSort, WindowKind,
    };

    #[test]
    fn window_codec_roundtrip() {
        for w in [
            WindowKind::Gaussian,
            WindowKind::RaisedCosine,
            WindowKind::Blackman,
            WindowKind::Kaiser,
        ] {
            assert_eq!(window_from_u8(window_to_u8(w)), w);
        }
        assert_eq!(window_from_u8(99), WindowKind::Gaussian);
    }

    #[test]
    fn channel_filter_codec_roundtrip() {
        for k in [ChannelFilterKind::LinearFir, ChannelFilterKind::Iir2Pole] {
            assert_eq!(channel_filter_from_u8(channel_filter_to_u8(k)), k);
        }
        assert_eq!(
            channel_filter_from_u8(9),
            ChannelFilterKind::LinearFir
        );
    }

    #[test]
    fn agc_mode_codec_roundtrip() {
        for m in [AgcMode::Envelope, AgcMode::Hang, AgcMode::DualLoop] {
            assert_eq!(agc_mode_from_u8(agc_mode_to_u8(m)), m);
        }
        assert_eq!(agc_mode_from_u8(9), AgcMode::Envelope);
    }

    #[test]
    fn spot_sort_codec_roundtrip() {
        for s in [
            SpotSort::SnrDesc,
            SpotSort::Frequency,
            SpotSort::LastHeard,
            SpotSort::Callsign,
        ] {
            assert_eq!(spot_sort_from_u8(spot_sort_to_u8(s)), s);
        }
        assert_eq!(spot_sort_from_u8(9), SpotSort::SnrDesc);
    }

    #[test]
    fn skimmer_decoder_codec_roundtrip() {
        assert_eq!(
            skimmer_decoder_from_u8(skimmer_decoder_to_u8(SkimmerDecoderKind::Bigram)),
            SkimmerDecoderKind::Bigram
        );
        assert_eq!(
            skimmer_decoder_from_u8(skimmer_decoder_to_u8(SkimmerDecoderKind::Adaptive)),
            SkimmerDecoderKind::Adaptive
        );
        assert_eq!(
            skimmer_decoder_from_u8(0),
            SkimmerDecoderKind::Bigram
        );
    }

    #[test]
    fn normalize_waterfall_avg_only_allows_one_two_four() {
        assert_eq!(normalize_waterfall_avg(1), 1);
        assert_eq!(normalize_waterfall_avg(2), 2);
        assert_eq!(normalize_waterfall_avg(4), 4);
        assert_eq!(normalize_waterfall_avg(3), 1);
        assert_eq!(normalize_waterfall_avg(99), 1);
    }

    #[test]
    fn plot_action_changes_view_for_pan_and_zoom() {
        assert!(plot_action_changes_view(&PlotAction::PanViewDeltaHz(100.0)));
        assert!(plot_action_changes_view(&PlotAction::ZoomView(1.5)));
        assert!(plot_action_changes_view(&PlotAction::SetViewPanHz(500.0)));
        assert!(!plot_action_changes_view(&PlotAction::SetPassbandHz(200.0)));
        assert!(!plot_action_changes_view(&PlotAction::TuneDeltaHz(50.0)));
    }

    #[test]
    fn skimmer_config_from_settings_clamps_channels() {
        let mut s = AppSettings::default();
        s.skimmer_max_channels = 0;
        let cfg = skimmer_config_from_settings(&s);
        assert!(cfg.max_channels >= 1);
        assert_eq!(cfg.decoder_params.max_text_chars, s.skimmer_max_decode_chars.max(16));
    }
}
