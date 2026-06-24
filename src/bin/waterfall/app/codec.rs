//! Settings serialization helpers and small shared utilities.

use eframe::egui;
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

pub(crate) fn window_choice(
    ui: &mut egui::Ui,
    current: &mut WindowKind,
    kind: WindowKind,
    label: &str,
    tip: &str,
) {
    let r = ui.selectable_label(*current == kind, label);
    if r.clicked() {
        *current = kind;
    }
    r.on_hover_text(tip);
}

pub(crate) fn window_to_u8(w: WindowKind) -> u8 {
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
