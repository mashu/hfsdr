//! RF panadapter view geometry — one mapping for plots, waterfall, and hit-testing.
//!
//! Kiwi receivers can show a padded CW band overview wider than the IQ passband.
//! Local SDRs (Airspy, playback) always use the native IQ bandwidth with no padding.

use hfsdr::{
    kiwi_iq_half_hz, spectrum_view_mapping, waterfall_storage_mapping, SpectrumViewMapping,
};

use crate::interaction::PlotViewState;

/// RF span covered by the spectrum FFT (may be narrower than device IQ after decimation).
pub fn spectrum_plot_span_hz(spectrum_rate_hz: f32, iq_passband_hz: f32) -> f32 {
    if spectrum_rate_hz > 0.0 {
        spectrum_rate_hz
    } else {
        iq_passband_hz
    }
}

/// Visible RF passband width (Hz) for display clamping and axis labels.
pub fn iq_passband_hz(is_kiwi: bool, stats_passband: f32, device_rate: f32) -> f32 {
    if stats_passband > 0.0 {
        return stats_passband;
    }
    if is_kiwi && device_rate > 0.0 {
        kiwi_iq_half_hz(device_rate as u32) as f32 * 2.0
    } else {
        device_rate.max(1.0)
    }
}

/// Maximum view zoom-out: Kiwi band overview; local SDR stays at full IQ only.
pub fn max_zoom_out(is_kiwi: bool, iq_passband_hz: f32, band_overview_span_hz: f32) -> f32 {
    if !is_kiwi {
        return 1.0;
    }
    (band_overview_span_hz / iq_passband_hz.max(1.0)).max(1.0)
}

/// Build the spectrum view mapping shared by trace, waterfall, and mouse interaction.
pub fn build_spectrum_view(
    is_kiwi: bool,
    iq_passband_hz: f32,
    plot_span_hz: f32,
    band_overview_span_hz: f32,
    spectrum_rate_hz: f32,
    spectrum_zoomed: bool,
    plot: &PlotViewState,
) -> SpectrumViewMapping {
    let max_zoom = max_zoom_out(is_kiwi, iq_passband_hz, band_overview_span_hz);
    let view_span = plot.view_span_hz(plot_span_hz, max_zoom);
    spectrum_view_mapping(
        iq_passband_hz,
        spectrum_rate_hz,
        spectrum_zoomed,
        view_span,
        plot.pan_offset_hz,
        is_kiwi,
    )
}

/// Fixed full-band mapping for waterfall history rows (pan = 0; zoom is a viewport crop).
pub fn build_waterfall_storage_view(
    is_kiwi: bool,
    iq_passband_hz: f32,
    plot_span_hz: f32,
    band_overview_span_hz: f32,
    spectrum_rate_hz: f32,
) -> SpectrumViewMapping {
    waterfall_storage_mapping(
        iq_passband_hz,
        plot_span_hz,
        spectrum_rate_hz,
        band_overview_span_hz,
        is_kiwi,
    )
}
